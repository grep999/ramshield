t
    echo "Building project..."
    cargo build --all-targets || { echo "Error: Build failed"; exit 1; }
    
    Run tests
    echo "Running tests..."
    cargo test || { echo "Error: Tests failed"; exit 1; }
    
    Start server in background
    echo "Starting server..."
    ./target/release/ramshield ./config.stress.toml &
    
    Check health endpoint
    echo "Checking health endpoint..."
    curl -s http://127.0.0.1:7891/healthz | jq -e '.healthy == true' > /dev/null || { echo "Error: Health check failed"; exit 1; }
    
    Check metrics endpoint
    echo "Checking metrics endpoint..."
    curl -s http://127.0.0.1:7891/metrics | jq -e '.cpu_usage != null and .memory_usage_mb != null' > /dev/null || { echo "Error: Metrics check failed"; exit 1; }
    
    Check dashboard
    echo "Checking dashboard..."
    curl -s http://127.0.0.1:7891/dashboard | jq -e '.status == "ok"' > /dev/null || { echo "Error: Dashboard check failed"; exit 1; }
    
    Cleanup
    echo "Shutting down server..."
    pkill -f ramshield
    
    echo "All checks passed successfully!"
    
    
    The script performs the following verification steps:
    
    1. Builds the project
    2. Runs all tests
    3. Starts server in background
    4. Verifies health endpoint
    5. Checks metrics endpoint
    6. Validates dashboard
    7. Cleanup
    
    

