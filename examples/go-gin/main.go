package main

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
)

type OrderRequest struct {
	ItemID   string `json:"item_id"`
	Quantity int    `json:"quantity"`
}

func main() {
	r := gin.Default()

	r.GET("/api/v1/health", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"status": "healthy",
			"uptime": time.Now().Format(time.RFC3339),
		})
	})

	r.POST("/api/v1/checkout", func(c *gin.Context) {
		var req OrderRequest
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		c.JSON(http.StatusCreated, gin.H{
			"order_id":   uuid.New().String(),
			"item_id":    req.ItemID,
			"quantity":   req.Quantity,
			"created_at": time.Now().UTC().Format(time.RFC3339),
		})
	})

	r.Run("127.0.0.1:8080")
}
