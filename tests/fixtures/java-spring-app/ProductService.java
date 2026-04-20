package com.example.service;

import java.util.List;
import java.util.Optional;
import java.util.stream.Collectors;

/**
 * Service for managing product inventory.
 */
public class ProductService {

    private final ProductRepository repository;
    private final PricingEngine pricingEngine;

    public ProductService(ProductRepository repository, PricingEngine pricingEngine) {
        this.repository = repository;
        this.pricingEngine = pricingEngine;
    }

    /**
     * Find a product by its unique identifier.
     *
     * @param id the product ID
     * @return the product, or empty if not found
     */
    public Optional<Product> findById(Long id) {
        if (id == null || id <= 0) {
            throw new IllegalArgumentException("Product ID must be positive");
        }
        return repository.findById(id);
    }

    /**
     * Create a new product with validated pricing.
     *
     * @param name  product name
     * @param price base price in cents
     * @return the created product
     */
    public Product createProduct(String name, int price) {
        if (name == null || name.isBlank()) {
            throw new IllegalArgumentException("Product name cannot be blank");
        }
        if (price < 0) {
            throw new IllegalArgumentException("Price cannot be negative");
        }

        int finalPrice = pricingEngine.applyTax(price);
        Product product = new Product(null, name, finalPrice, true);
        return repository.save(product);
    }

    /**
     * List all products that are currently in stock.
     */
    public List<Product> listAvailable() {
        return repository.findAll().stream()
                .filter(Product::isAvailable)
                .collect(Collectors.toList());
    }

    /**
     * Apply a percentage discount to a product.
     *
     * @param productId      the product to discount
     * @param discountPercent discount percentage (0-100)
     * @return updated product
     */
    public Product applyDiscount(Long productId, int discountPercent) {
        if (discountPercent < 0 || discountPercent > 100) {
            throw new IllegalArgumentException("Discount must be between 0 and 100");
        }

        Product product = findById(productId)
                .orElseThrow(() -> new RuntimeException("Product not found: " + productId));

        int discountedPrice = product.getPrice() * (100 - discountPercent) / 100;
        product.setPrice(discountedPrice);
        return repository.save(product);
    }

    /**
     * Deactivate a product (soft delete).
     */
    public boolean deactivate(Long productId) {
        Optional<Product> product = findById(productId);
        if (product.isEmpty()) {
            return false;
        }
        product.get().setAvailable(false);
        repository.save(product.get());
        return true;
    }
}

interface ProductRepository {
    Optional<Product> findById(Long id);
    List<Product> findAll();
    Product save(Product product);
}

interface PricingEngine {
    int applyTax(int basePrice);
}

class Product {
    private Long id;
    private String name;
    private int price;
    private boolean available;

    public Product(Long id, String name, int price, boolean available) {
        this.id = id;
        this.name = name;
        this.price = price;
        this.available = available;
    }

    public Long getId() { return id; }
    public String getName() { return name; }
    public int getPrice() { return price; }
    public boolean isAvailable() { return available; }
    public void setPrice(int price) { this.price = price; }
    public void setAvailable(boolean available) { this.available = available; }
}
