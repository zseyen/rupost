# AI & Realtime Collaboration API Spec

本规范定义了场景 C 的核心协议契约：AI 聊天流式补全（SSE）与多端画布协同（WebSocket）。

## 1. AI Chat Completions (SSE)
```http
POST /api/v1/chat/completions
Content-Type: application/json
Accept: text/event-stream
@sse
@sse_timeout 5s
@stream_to test_scratch/scenario_c_chat.txt

{
  "model": "gpt-4o",
  "messages": [
    {
      "role": "user",
      "content": "Hello Rupost"
    }
  ],
  "stream": true
}

@mock-when header.X-RateLimit-Test == 1
HTTP/1.1 429 Too Many Requests
Content-Type: application/json

{
  "error": {
    "code": "rate_limit_exceeded",
    "message": "Rate limit exceeded"
  }
}

@mock-default
HTTP/1.1 200 OK
Content-Type: text/event-stream

data: {"id":"1","choices":[{"delta":{"content":"Hello"}}]}

data: {"id":"2","choices":[{"delta":{"content":" from"}}]}

data: {"id":"3","choices":[{"delta":{"content":" Rupost!"}}]}

data: [DONE]
```

## 2. Realtime Canvas Collaboration (WebSocket)
```http
GET /api/v1/collaboration/canvas_101
@websocket
@timeout 5s
@decoder json

SEND {"action":"join","user":"designer_a"}
EXPECT {"event":"user_joined","user":"designer_a"}
SEND {"action":"draw","shape":"rectangle","x":100,"y":200}
EXPECT {"event":"shape_added","shape":"rectangle"}
CLOSE
```
