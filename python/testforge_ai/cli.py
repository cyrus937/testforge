"""
TestForge AI CLI — Python-side commands for embeddings and AI features.

Usage:
    testforge-ai embed              Compute embeddings for all indexed symbols
    testforge-ai search <query>     Semantic search using embeddings
    testforge-ai stats              Show embedding cache statistics

These commands complement the main Rust CLI by adding AI capabilities
that require Python's ML ecosystem.
"""

from __future__ import annotations

import argparse
import json
import logging
import sys
from pathlib import Path

logger = logging.getLogger("testforge_ai")


def cmd_embed(args: argparse.Namespace) -> int:
    """Compute embeddings for all indexed symbols."""
    from testforge_ai.embeddings.pipeline import EmbeddingPipeline, EmbeddingPipelineConfig

    project_root = Path(args.project).resolve()

    config = EmbeddingPipelineConfig(
        provider=args.provider,
        model=args.model,
        batch_size=args.batch_size,
    )

    print(f"  Computing embeddings for {project_root}...")
    print(f"  Provider: {config.provider} ({config.model})")
    print()

    try:
        pipeline = EmbeddingPipeline(project_root, config)
        report = pipeline.run()

        print(f"  ✓ {report.summary}")
        print()
        print(f"  Dimension:   {report.dimension}")
        print(f"  Embedded:    {report.embedded}")
        print(f"  From cache:  {report.cached}")
        if report.errors > 0:
            print(f"  Errors:      {report.errors}")
        print()
        return 0

    except FileNotFoundError as e:
        print(f"  ✗ {e}", file=sys.stderr)
        print("  Run `testforge init && testforge index .` first.", file=sys.stderr)
        return 1
    except Exception as e:
        print(f"  ✗ Error: {e}", file=sys.stderr)
        return 1


def cmd_search(args: argparse.Namespace) -> int:
    """Semantic search using query embeddings."""
    from testforge_ai.embeddings.pipeline import EmbeddingPipeline, EmbeddingPipelineConfig
    from testforge_ai.bridge import TestForgeBridge

    project_root = Path(args.project).resolve()
    query = args.query

    try:
        config = EmbeddingPipelineConfig(provider="local")
        pipeline = EmbeddingPipeline(project_root, config)

        # Embed the query
        print(f"  Searching for: \"{query}\"")
        query_vec = pipeline.embed_query(query)

        # Get all symbols and compute similarity
        bridge = TestForgeBridge(project_root)
        symbols = bridge.get_all_symbols()

        if not symbols:
            print("  No symbols indexed. Run `testforge index .` first.")
            return 1

        # Embed and score each symbol
        scored: list[tuple[float, object]] = []
        for sym in symbols:
            try:
                sym_vec = pipeline.embed_symbol(sym)
                # Cosine similarity (vectors are normalized)
                score = sum(a * b for a, b in zip(query_vec, sym_vec))
                scored.append((score, sym))
            except Exception:
                continue

        # Sort by score descending
        scored.sort(key=lambda x: x[0], reverse=True)
        top = scored[: args.limit]

        if args.format == "json":
            results = [
                {
                    "score": round(s, 4),
                    "name": sym.qualified_name,
                    "kind": sym.kind,
                    "file": sym.file_path,
                    "lines": f"{sym.start_line}-{sym.end_line}",
                }
                for s, sym in top
            ]
            print(json.dumps(results, indent=2))
            return 0

        print(f"\n  Found {len(top)} results:\n")
        for i, (score, sym) in enumerate(top):
            print(f"  {i + 1:>2}. [{score:.3f}]  {sym.kind:<10} {sym.qualified_name}")
            print(f"       ↳ {sym.file_path}:{sym.start_line}-{sym.end_line}")
            if sym.docstring:
                doc = sym.docstring[:70] + "..." if len(sym.docstring) > 70 else sym.docstring
                print(f"       {doc}")
            print()

        return 0

    except FileNotFoundError as e:
        print(f"  ✗ {e}", file=sys.stderr)
        return 1
    except Exception as e:
        print(f"  ✗ Error: {e}", file=sys.stderr)
        return 1


def cmd_stats(args: argparse.Namespace) -> int:
    """Show embedding cache statistics."""
    project_root = Path(args.project).resolve()
    cache_dir = project_root / ".testforge" / "cache" / "embeddings"

    if not cache_dir.exists():
        print("  No embedding cache found. Run `testforge-ai embed` first.")
        return 0

    npy_files = list(cache_dir.glob("*.npy"))
    total_size = sum(f.stat().st_size for f in npy_files)

    print(f"\n  Embedding Cache — {project_root}")
    print()
    print(f"  Cached entries:  {len(npy_files)}")
    print(f"  Cache size:      {total_size / 1024:.1f} KB")
    print(f"  Cache directory: {cache_dir}")

    # Check vector store
    vec_file = project_root / ".testforge" / "search" / "vectors" / "vectors.bin"
    if vec_file.exists():
        vec_size = vec_file.stat().st_size
        print(f"\n  Vector store:    {vec_size / 1024:.1f} KB")
    else:
        print(f"\n  Vector store:    not built yet")

    print()
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        prog="testforge-ai",
        description="TestForge AI layer — embeddings and intelligent search",
    )
    parser.add_argument(
        "-p", "--project",
        default=".",
        help="Project root directory (default: current directory)",
    )
    parser.add_argument(
        "-v", "--verbose",
        action="count",
        default=0,
        help="Increase verbosity",
    )

    subparsers = parser.add_subparsers(dest="command", required=True)

    # embed
    embed_parser = subparsers.add_parser("embed", help="Compute embeddings")
    embed_parser.add_argument(
        "--provider", default="local",
        choices=["local", "openai"],
        help="Embedding provider (default: local)",
    )
    embed_parser.add_argument(
        "--model", default="all-MiniLM-L6-v2",
        help="Model name (default: all-MiniLM-L6-v2)",
    )
    embed_parser.add_argument(
        "--batch-size", type=int, default=64,
        help="Batch size (default: 64)",
    )

    # search
    search_parser = subparsers.add_parser("search", help="Semantic search")
    search_parser.add_argument("query", help="Search query")
    search_parser.add_argument(
        "-l", "--limit", type=int, default=10,
        help="Max results (default: 10)",
    )
    search_parser.add_argument(
        "-f", "--format", default="pretty",
        choices=["pretty", "json"],
        help="Output format",
    )

    # stats
    subparsers.add_parser("stats", help="Cache statistics")

    args = parser.parse_args()

    # Setup logging
    level = {0: logging.WARNING, 1: logging.INFO}.get(args.verbose, logging.DEBUG)
    logging.basicConfig(
        level=level,
        format="  %(levelname)s %(name)s: %(message)s",
    )

    commands = {
        "embed": cmd_embed,
        "search": cmd_search,
        "stats": cmd_stats,
    }

    return commands[args.command](args)


if __name__ == "__main__":
    sys.exit(main())