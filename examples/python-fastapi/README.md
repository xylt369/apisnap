# FastAPI + ApiSnap Example

This example demonstrates zero-code API regression testing for a Python FastAPI application.

## 🚀 How to Run

1. Install dependencies:
   ```bash
   pip install fastapi uvicorn
   ```

2. Start the FastAPI server:
   ```bash
   python main.py
   ```

3. In another terminal, record snapshots:
   ```bash
   apisnap record
   ```

4. Run regression tests in 10ms:
   ```bash
   apisnap test
   ```
