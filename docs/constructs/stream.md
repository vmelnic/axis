# STREAM

Real-time event delivery over WebSocket or Server-Sent Events (SSE).

## Syntax

```axis
STREAM <name> <transport> <path>
  [REALM <realm_name>]
  [AUTH <auth_spec>]

  EVENT <event_name>
    <field1> <type>
    [field2 type...]

  [RECEIVE ON <event_name>
    <flow_steps>...]
```

## Transport Types

| Transport | Keyword | Protocol | Direction |
|-----------|---------|----------|-----------|
| WebSocket | `ws` | Full-duplex WebSocket | Bidirectional |
| Server-Sent Events | `sse` | HTTP streaming | Server to client only |

## Example

```axis
STREAM order_updates ws /ws/orders
  REALM shop_api
  AUTH session

  EVENT order_created
    order_id UUID
    status STRING
    total DECIMAL

  EVENT order_status_changed
    order_id UUID
    old_status STRING
    new_status STRING

  RECEIVE ON place_order
    LET order
      FETCH orders
        FILTER id EQ body.order_id
      OR 404
    INSERT order_events
      order_id order.id
      event_type placed
    AS event

STREAM notifications sse /sse/notifications
  REALM api
  AUTH session

  EVENT new_message
    from_user_id UUID
    message_text TEXT

  EVENT booking_update
    booking_id UUID
    status STRING
```

## EVENT

Declares an event type that can be sent to connected clients. Each event has a name and typed fields.

Events are broadcast to all connected clients subscribed to this stream. Clients receive only events whose name matches their subscription.

## RECEIVE ON

WebSocket streams can receive messages from clients. `RECEIVE ON <event_name>` defines a handler that processes incoming messages using flow steps (LET, FETCH, INSERT, UPDATE, etc.).

SSE streams are server-to-client only and do not support RECEIVE.

## Runtime Behavior

### WebSocket

1. Client connects to the WebSocket endpoint.
2. Auth is checked before the upgrade completes.
3. The server subscribes the connection to the stream's broadcast channel.
4. Incoming messages are dispatched to matching RECEIVE handlers.
5. Outgoing events are filtered by event name and sent as JSON.

### SSE

1. Client connects to the SSE endpoint.
2. Auth is checked before streaming begins.
3. Events are sent in SSE format: `event: <name>\ndata: <json>\n\n`.

### Broadcasting

Effects triggered by flows can broadcast to streams. When an `EFFECT` fires, the event is published to the stream's broadcast channel, and all connected clients receive it.

## Compiler Checks

- Stream realm must exist if declared.
- EVENT field types are validated.
- RECEIVE handler steps follow flow operation rules.
