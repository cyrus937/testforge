//! In-memory vector store with cosine similarity search.
//!
//! Stores dense embedding vectors alongside symbol UUIDs and supports
//! fast nearest-neighbor retrieval. Uses a flat index with SIMD-friendly
//! layout for maximum throughput on datasets up to ~100K vectors.
//!
//! ## Design Decisions
//!
//! - **Flat index** — brute-force search with optimized inner product.
//!   Correct and fast up to ~100K vectors (sub-10ms on modern hardware).
//!   HNSW can be added as a backend later without changing the API.
//! - **Normalized storage** — all vectors are L2-normalized on insert,
//!   so cosine similarity reduces to dot product.
//! - **Persistence** — vectors are serialized to disk as a compact binary
//!   format for fast startup.

use std::path::{Path, PathBuf};

use ordered_float::OrderedFloat;
use parking_lot::RwLock;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use testforge_core::{Result, TestForgeError};
use tracing::{debug, info};
use uuid::Uuid;

/// A single match from vector search.
#[derive(Debug, Clone)]
pub struct VectorMatch {
    /// UUID of the matched symbol.
    pub id: Uuid,
    /// Cosine similarity score in [-1.0, 1.0].
    pub score: f32,
    /// Rank position (0-based).
    pub rank: usize,
}

/// On-disk header for the vector store file.
#[derive(Debug, Serialize, Deserialize)]
struct StoreHeader {
    version: u32,
    dimension: u32,
    count: u64,
}

/// Thread-safe in-memory vector store.
pub struct VectorStore {
    inner: RwLock<VectorStoreInner>,
    persist_path: Option<PathBuf>,
}

struct VectorStoreInner {
    /// Embedding dimension (e.g., 384 for MiniLM).
    dimension: usize,
    /// Symbol UUIDs, parallel to the vectors matrix.
    ids: Vec<Uuid>,
    /// Flat matrix of L2-normalized vectors, stored row-major.
    /// Layout: [vec0_dim0, vec0_dim1, ..., vec1_dim0, vec1_dim1, ...]
    vectors: Vec<f32>,
}

impl VectorStore {
    /// Open or create a vector store at the given directory.
    ///
    /// If a persisted store exists, it's loaded into memory.
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let persist_path = dir.join("vectors.bin");

        let inner = if persist_path.exists() {
            Self::load_from_disk(&persist_path)?
        } else {
            VectorStoreInner {
                dimension: 0,
                ids: Vec::new(),
                vectors: Vec::new(),
            }
        };

        info!(
            count = inner.ids.len(),
            dimension = inner.dimension,
            "Vector store loaded"
        );

        Ok(Self {
            inner: RwLock::new(inner),
            persist_path: Some(persist_path),
        })
    }

    /// Create an in-memory-only vector store (for testing).
    #[cfg(test)]
    pub fn in_memory(dimension: usize) -> Self {
        Self {
            inner: RwLock::new(VectorStoreInner {
                dimension,
                ids: Vec::new(),
                vectors: Vec::new(),
            }),
            persist_path: None,
        }
    }

    /// Add a vector to the store.
    ///
    /// The vector is L2-normalized before storage. If the store is empty,
    /// the dimension is inferred from the first vector inserted.
    pub fn add(&self, id: Uuid, vector: &[f32]) -> Result<()> {
        let mut inner = self.inner.write();

        // Infer dimension from first vector
        if inner.dimension == 0 {
            inner.dimension = vector.len();
            debug!(dimension = inner.dimension, "Vector dimension set");
        }

        if vector.len() != inner.dimension {
            return Err(TestForgeError::internal(format!(
                "Vector dimension mismatch: expected {}, got {}",
                inner.dimension,
                vector.len()
            )));
        }

        // L2-normalize
        let normalized = l2_normalize(vector);

        // Check if ID already exists (update in-place)
        if let Some(pos) = inner.ids.iter().position(|existing| *existing == id) {
            let start = pos * inner.dimension;
            let dimension = inner.dimension;
            inner.vectors[start..start + dimension].copy_from_slice(&normalized);
            return Ok(());
        }

        inner.ids.push(id);
        inner.vectors.extend_from_slice(&normalized);

        Ok(())
    }

    /// Batch-add vectors for better performance.
    pub fn add_batch(&self, entries: &[(Uuid, Vec<f32>)]) -> Result<()> {
        let mut inner = self.inner.write();

        if entries.is_empty() {
            return Ok(());
        }

        // Infer dimension
        if inner.dimension == 0 {
            inner.dimension = entries[0].1.len();
        }

        inner.ids.reserve(entries.len());
        let additional_size = entries.len() * inner.dimension;
        inner.vectors.reserve(additional_size);

        for (id, vec) in entries {
            if vec.len() != inner.dimension {
                return Err(TestForgeError::internal(format!(
                    "Dimension mismatch for {}: expected {}, got {}",
                    id,
                    inner.dimension,
                    vec.len()
                )));
            }

            let normalized = l2_normalize(vec);

            // Update or append
            if let Some(pos) = inner.ids.iter().position(|existing| *existing == *id) {
                let start = pos * inner.dimension;
                let dimension = inner.dimension;
                inner.vectors[start..start + dimension].copy_from_slice(&normalized);
            } else {
                inner.ids.push(*id);
                inner.vectors.extend_from_slice(&normalized);
            }
        }

        Ok(())
    }

    /// Find the top-K most similar vectors by cosine similarity.
    ///
    /// The query vector is L2-normalized internally.
    /// Uses parallel computation for large stores (>1000 vectors).
    pub fn search(&self, query: &[f32], limit: usize) -> Result<Vec<VectorMatch>> {
        let inner = self.inner.read();

        if inner.ids.is_empty() {
            return Ok(Vec::new());
        }

        if inner.dimension == 0 {
            return Ok(Vec::new());
        }

        if query.len() != inner.dimension {
            return Err(TestForgeError::internal(format!(
                "Query dimension mismatch: expected {}, got {}",
                inner.dimension,
                query.len()
            )));
        }

        let normalized_query = l2_normalize(query);
        let n = inner.ids.len();
        let dim = inner.dimension;

        // Compute all dot products (= cosine similarity for normalized vectors)
        let scores: Vec<(usize, f32)> = if n > 1000 {
            // Parallel for large stores
            (0..n)
                .into_par_iter()
                .map(|i| {
                    let start = i * dim;
                    let vec_slice = &inner.vectors[start..start + dim];
                    let score = dot_product(&normalized_query, vec_slice);
                    (i, score)
                })
                .collect()
        } else {
            // Sequential for small stores
            (0..n)
                .map(|i| {
                    let start = i * dim;
                    let vec_slice = &inner.vectors[start..start + dim];
                    let score = dot_product(&normalized_query, vec_slice);
                    (i, score)
                })
                .collect()
        };

        // Partial sort to get top-K (more efficient than full sort)
        let mut scored: Vec<(usize, OrderedFloat<f32>)> = scores
            .into_iter()
            .map(|(i, s)| (i, OrderedFloat(s)))
            .collect();

        // Sort descending by score
        scored.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        scored.truncate(limit);

        let matches = scored
            .into_iter()
            .enumerate()
            .map(|(rank, (idx, score))| VectorMatch {
                id: inner.ids[idx],
                score: score.into_inner(),
                rank,
            })
            .collect();

        Ok(matches)
    }

    /// Remove a vector by ID.
    pub fn remove(&self, id: &Uuid) -> bool {
        let mut inner = self.inner.write();
        if let Some(pos) = inner.ids.iter().position(|existing| *existing == *id) {
            inner.ids.swap_remove(pos);
            let dim = inner.dimension;
            let last = inner.ids.len(); // after swap_remove, this is the new len
            if pos < last {
                // Copy the swapped vector to the removed position
                let src_start = last * dim;
                let dst_start = pos * dim;
                for d in 0..dim {
                    inner.vectors[dst_start + d] = inner.vectors[src_start + d];
                }
            }
            inner.vectors.truncate(last * dim);
            true
        } else {
            false
        }
    }

    /// Number of vectors in the store.
    pub fn len(&self) -> usize {
        self.inner.read().ids.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.read().ids.is_empty()
    }

    /// Embedding dimension (0 if no vectors have been added yet).
    pub fn dimension(&self) -> usize {
        self.inner.read().dimension
    }

    /// Clear all vectors.
    pub fn clear(&self) -> Result<()> {
        let mut inner = self.inner.write();
        inner.ids.clear();
        inner.vectors.clear();
        // Keep dimension so new vectors must match
        Ok(())
    }

    /// Persist the store to disk.
    pub fn save(&self) -> Result<()> {
        let path = match &self.persist_path {
            Some(p) => p,
            None => return Ok(()),
        };

        let inner = self.inner.read();
        let header = StoreHeader {
            version: 1,
            dimension: inner.dimension as u32,
            count: inner.ids.len() as u64,
        };

        let header_bytes = serde_json::to_vec(&header)
            .map_err(|e| TestForgeError::internal(format!("Failed to serialize header: {e}")))?;

        let mut data = Vec::new();

        // Header length (4 bytes) + header + IDs + vectors
        let header_len = header_bytes.len() as u32;
        data.extend_from_slice(&header_len.to_le_bytes());
        data.extend_from_slice(&header_bytes);

        // IDs as raw bytes (16 bytes each)
        for id in &inner.ids {
            data.extend_from_slice(id.as_bytes());
        }

        // Vectors as raw f32 bytes
        let vec_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                inner.vectors.as_ptr() as *const u8,
                inner.vectors.len() * std::mem::size_of::<f32>(),
            )
        };
        data.extend_from_slice(vec_bytes);

        std::fs::write(path, &data)?;

        info!(
            count = inner.ids.len(),
            bytes = data.len(),
            "Vector store persisted"
        );

        Ok(())
    }

    /// Load the store from disk.
    fn load_from_disk(path: &Path) -> Result<VectorStoreInner> {
        let data = std::fs::read(path)?;
        if data.len() < 4 {
            return Err(TestForgeError::internal("Vector store file too small"));
        }

        // Read header length
        let header_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let header_end = 4 + header_len;

        if data.len() < header_end {
            return Err(TestForgeError::internal("Truncated vector store header"));
        }

        let header: StoreHeader = serde_json::from_slice(&data[4..header_end])
            .map_err(|e| TestForgeError::internal(format!("Invalid header: {e}")))?;

        let dim = header.dimension as usize;
        let count = header.count as usize;

        // Read IDs
        let ids_start = header_end;
        let ids_end = ids_start + count * 16;
        if data.len() < ids_end {
            return Err(TestForgeError::internal("Truncated ID section"));
        }

        let mut ids = Vec::with_capacity(count);
        for i in 0..count {
            let offset = ids_start + i * 16;
            let bytes: [u8; 16] = data[offset..offset + 16]
                .try_into()
                .map_err(|_| TestForgeError::internal("Invalid UUID bytes"))?;
            ids.push(Uuid::from_bytes(bytes));
        }

        // Read vectors
        let vecs_start = ids_end;
        let vecs_bytes = count * dim * std::mem::size_of::<f32>();
        if data.len() < vecs_start + vecs_bytes {
            return Err(TestForgeError::internal("Truncated vector section"));
        }

        let mut vectors = vec![0.0f32; count * dim];
        let src = &data[vecs_start..vecs_start + vecs_bytes];
        unsafe {
            std::ptr::copy_nonoverlapping(
                src.as_ptr(),
                vectors.as_mut_ptr() as *mut u8,
                vecs_bytes,
            );
        }

        debug!(
            count,
            dimension = dim,
            "Loaded vectors from {}",
            path.display()
        );

        Ok(VectorStoreInner {
            dimension: dim,
            ids,
            vectors,
        })
    }
}

// ── Math Primitives ──────────────────────────────────────────────────

/// Compute the dot product of two slices.
///
/// For L2-normalized vectors, this equals cosine similarity.
#[inline]
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// L2-normalize a vector. Returns a zero vector if the input has zero norm.
fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < f32::EPSILON {
        vec![0.0; v.len()]
    } else {
        v.iter().map(|x| x / norm).collect()
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn random_vector(dim: usize, seed: u64) -> Vec<f32> {
        // Simple deterministic pseudo-random
        let mut v = Vec::with_capacity(dim);
        let mut state = seed;
        for _ in 0..dim {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let val = ((state >> 33) as f32) / (u32::MAX as f32) - 0.5;
            v.push(val);
        }
        v
    }

    #[test]
    fn add_and_search_single_vector() {
        let store = VectorStore::in_memory(4);
        let id = Uuid::new_v4();
        let vec = vec![1.0, 0.0, 0.0, 0.0];

        store.add(id, &vec).unwrap();
        assert_eq!(store.len(), 1);

        let results = store.search(&vec, 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
        assert!((results[0].score - 1.0).abs() < 0.01);
    }

    #[test]
    fn search_returns_most_similar() {
        let store = VectorStore::in_memory(3);

        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let id_c = Uuid::new_v4();

        store.add(id_a, &[1.0, 0.0, 0.0]).unwrap();
        store.add(id_b, &[0.9, 0.1, 0.0]).unwrap(); // most similar to a
        store.add(id_c, &[0.0, 0.0, 1.0]).unwrap(); // least similar

        let results = store.search(&[1.0, 0.0, 0.0], 3).unwrap();
        assert_eq!(results[0].id, id_a); // exact match
        assert_eq!(results[1].id, id_b); // close
        assert_eq!(results[2].id, id_c); // far
    }

    #[test]
    fn dimension_mismatch_returns_error() {
        let store = VectorStore::in_memory(3);
        store.add(Uuid::new_v4(), &[1.0, 0.0, 0.0]).unwrap();

        let result = store.add(Uuid::new_v4(), &[1.0, 0.0]);
        assert!(result.is_err());
    }

    #[test]
    fn update_existing_vector() {
        let store = VectorStore::in_memory(3);
        let id = Uuid::new_v4();

        store.add(id, &[1.0, 0.0, 0.0]).unwrap();
        store.add(id, &[0.0, 1.0, 0.0]).unwrap(); // update

        assert_eq!(store.len(), 1); // no duplicate

        let results = store.search(&[0.0, 1.0, 0.0], 1).unwrap();
        assert_eq!(results[0].id, id);
        assert!(results[0].score > 0.99); // should match updated vector
    }

    #[test]
    fn remove_vector() {
        let store = VectorStore::in_memory(3);
        let id = Uuid::new_v4();

        store.add(id, &[1.0, 0.0, 0.0]).unwrap();
        assert!(store.remove(&id));
        assert_eq!(store.len(), 0);
        assert!(!store.remove(&id)); // already removed
    }

    #[test]
    fn clear_removes_all() {
        let store = VectorStore::in_memory(3);
        for _ in 0..10 {
            store.add(Uuid::new_v4(), &[1.0, 0.0, 0.0]).unwrap();
        }
        assert_eq!(store.len(), 10);
        store.clear().unwrap();
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn batch_add() {
        let store = VectorStore::in_memory(4);
        let entries: Vec<_> = (0..100)
            .map(|i| (Uuid::new_v4(), random_vector(4, i)))
            .collect();

        store.add_batch(&entries).unwrap();
        assert_eq!(store.len(), 100);
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = VectorStore::open(dir.path()).unwrap();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        store.add(id1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        store.add(id2, &[0.0, 1.0, 0.0, 0.0]).unwrap();
        store.save().unwrap();

        // Reopen
        let store2 = VectorStore::open(dir.path()).unwrap();
        assert_eq!(store2.len(), 2);
        assert_eq!(store2.dimension(), 4);

        let results = store2.search(&[1.0, 0.0, 0.0, 0.0], 1).unwrap();
        assert_eq!(results[0].id, id1);
    }

    #[test]
    fn search_empty_store() {
        let store = VectorStore::in_memory(3);
        let results = store.search(&[1.0, 0.0, 0.0], 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn dot_product_correctness() {
        assert!((dot_product(&[1.0, 0.0], &[0.0, 1.0]) - 0.0).abs() < 1e-6);
        assert!((dot_product(&[1.0, 2.0], &[3.0, 4.0]) - 11.0).abs() < 1e-6);
        assert!((dot_product(&[0.6, 0.8], &[0.6, 0.8]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_unit_vector() {
        let v = l2_normalize(&[3.0, 4.0]);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
        assert!((v[0] - 0.6).abs() < 1e-5);
        assert!((v[1] - 0.8).abs() < 1e-5);
    }

    #[test]
    fn l2_normalize_zero_vector() {
        let v = l2_normalize(&[0.0, 0.0, 0.0]);
        assert!(v.iter().all(|x| *x == 0.0));
    }
}