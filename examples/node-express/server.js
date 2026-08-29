const express = require('express');
const crypto = require('crypto');

const app = express();
app.use(express.json());

app.get('/api/v1/session', (req, res) => {
  res.json({
    status: 'authenticated',
    session_id: crypto.randomUUID(),
    created_at: new Date().toISOString(),
    expires_in: 3600,
  });
});

app.post('/api/v1/messages', (req, res) => {
  res.status(201).json({
    id: crypto.randomUUID(),
    text: req.body.text || 'Hello World',
    created_at: new Date().toISOString(),
  });
});

app.listen(3000, () => {
  console.log('Express app listening on http://127.0.0.1:3000');
});
