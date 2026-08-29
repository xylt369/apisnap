# Go (Gin) + ApiSnap Example

This example demonstrates zero-code API regression testing for a Go Gin microservice.

## 🚀 How to Run

1. Start the Go server:
   ```bash
   go run main.go
   ```

2. In another terminal, record initial snapshots:
   ```bash
   apisnap record
   ```

3. Run snapshot regression test:
   ```bash
   apisnap test
   ```
