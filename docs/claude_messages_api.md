# Claude Messages API - Create a Message

## API Endpoint

**POST** `/v1/messages`

## Description

Send a structured list of input messages with text and/or image content, and the model will generate the next message in the conversation. The Messages API can be used for either single queries or stateless multi-turn conversations.

## Request Headers

| Header | Required | Description |
|--------|----------|-------------|
| `x-api-key` | Yes | Your API key |
| `anthropic-version` | Yes | API version (e.g., "2023-06-01") |
| `Content-Type` | Yes | Must be `application/json` |

## Request Body Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `model` | string | Yes | Model ID to use (e.g., claude-opus-4-6, claude-sonnet-4-6) |
| `messages` | array | Yes | Input messages array with role and content |
| `max_tokens` | number | Yes | Max tokens to generate (min: 1) |
| `system` | string or array | No | System prompt |
| `temperature` | number | No | Sampling temperature (0-1, default varies by model) |
| `top_p` | number | No | Nucleus sampling (0-1) |
| `top_k` | number | No | Top-k sampling |
| `stop_sequences` | array | No | Custom stop sequences |
| `stream` | boolean | No | Enable streaming |
| `tools` | array | No | Available tools for the model |
| `tool_choice` | object | No | Control which tool to use |
| `metadata` | object | No | Request metadata |
| `cache_control` | object | No | Ephemeral cache control |

## MessageParam Structure

Each message in the `messages` array must have:

```json
{
  "role": "user" | "assistant",
  "content": string | ContentBlock[]
}
```

## ContentBlock Types

### Text Content
```json
{ "type": "text", "text": "Hello!" }
```

### Image Content  
```json
{
  "type": "image",
  "source": {
    "type": "base64" | "url",
    "media_type": "image/png" | "image/jpeg" | "image/gif" | "image/webp",
    "data": "base64..." | "url"
  }
}
```

### Cache Control
```json
{ "type": "cache_control", "ttl": "5m" | "1h" }
```

## Response 200

```json
{
  "id": "msg_013Zva2CMHLNnXjNJJKqJ2EF",
  "container": {
    "id": "id",
    "expires_at": "2019-12-27T18:11:19.117Z"
  },
  "content": [
    {
      "citations": [
        {
          "cited_text": "cited_text",
          "document_index": 0,
          "document_title": "document_title",
          "end_char_index": 0,
          "file_id": "file_id",
          "start_char_index": 0,
          "type": "char_location"
        }
      ],
      "text": "Hi! My name is Claude.",
      "type": "text"
    }
  ],
  "model": "claude-opus-4-6",
  "role": "assistant",
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "type": "message",
  "usage": {
    "cache_creation": {
      "ephemeral_1h_input_tokens": 0,
      "ephemeral_5m_input_tokens": 0
    },
    "cache_creation_input_tokens": 2051,
    "cache_read_input_tokens": 2051,
    "inference_geo": "inference_geo",
    "input_tokens": 2095,
    "output_tokens": 503,
    "server_tool_use": {
      "web_fetch_requests": 2,
      "web_search_requests": 0
    },
    "service_tier": "standard"
  }
}
```

## Response Structure

| Field | Type | Description |
|-------|------|-------------|
| id | string | Unique message ID (e.g., msg_013Zva2CMHLNnXjNJJKqJ2EF) |
| type | string | Always "message" |
| role | string | Always "assistant" |
| content | array | Response content blocks |
| model | string | Model used |
| stop_reason | string | Why generation stopped (end_turn, max_tokens, stop_sequence) |
| stop_sequence | string | Custom stop sequence if used |
| usage | object | Token usage information |
| container | object | Message container info |

## Usage Field Details

| Field | Type | Description |
|-------|------|-------------|
| usage.input_tokens | number | Input tokens used |
| usage.output_tokens | number | Output tokens generated |
| usage.cache_creation_input_tokens | number | Tokens used for cache creation |
| usage.cache_read_input_tokens | number | Tokens read from cache |

## Usage Example

```bash
curl -X POST https://api.anthropic.com/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-opus-4-6",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "Hello, Claude"}
    ]
  }'
```

### With System Prompt
```bash
curl -X POST https://api.anthropic.com/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-6",
    "max_tokens": 1024,
    "system": "You are a helpful assistant.",
    "messages": [
      {"role": "user", "content": "Hello"}
    ]
  }'
```

### With Tools
```bash
curl -X POST https://api.anthropic.com/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-6",
    "max_tokens": 1024,
    "tools": [
      {
        "name": "get_weather",
        "description": "Get weather for a location",
        "input_schema": {
          "type": "object",
          "properties": {
            "location": {"type": "string", "description": "City name"}
          },
          "required": ["location"]
        }
      }
    ],
    "messages": [
      {"role": "user", "content": "What is the weather in Tokyo?"}
    ]
  }'
```

## Error Responses

| Status | Error Type | Description |
|--------|------------|-------------|
| 400 | bad_request | Invalid request parameters |
| 401 | unauthorized | Invalid API key |
| 403 | permission_error | API key lacks permissions |
| 429 | rate_limit_error | Too many requests |
| 500 | internal_server_error | Server error |

## Available Models

- claude-opus-4-6 (latest)
- claude-sonnet-4-6 (latest)
- claude-haiku-3-5 (latest)
- claude-3-5-sonnet-20241022
- claude-3-opus-20240229
- claude-3-sonnet-20240229
- claude-3-haiku-20240307

## Stop Reasons

- `end_turn`: Model completed its turn normally
- `max_tokens`: Reached max_tokens limit
- `stop_sequence`: Custom stop sequence was generated
- `tool_use`: Model wants to use a tool (for streaming)
