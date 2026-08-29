from fastapi import FastAPI
from pydantic import BaseModel
import uuid
from datetime import datetime

app = FastAPI(title="FastAPI ApiSnap Example")

class OrderRequest(BaseModel):
    item_id: str
    quantity: int

@app.get("/api/v1/users/{user_id}")
def get_user(user_id: str):
    return {
        "status": "success",
        "data": {
            "user_id": str(uuid.uuid4()),
            "username": "developer_alice",
            "created_at": datetime.utcnow().isoformat() + "Z",
            "roles": ["developer", "tester"],
        }
    }

@app.post("/api/v1/orders", status_code=201)
def create_order(order: OrderRequest):
    return {
        "status": "created",
        "data": {
            "order_id": str(uuid.uuid4()),
            "item_id": order.item_id,
            "quantity": order.quantity,
            "created_at": datetime.utcnow().isoformat() + "Z",
        }
    }

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=8000)
