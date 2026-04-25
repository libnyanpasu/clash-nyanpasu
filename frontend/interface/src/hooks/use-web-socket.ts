import { useCallback, useEffect, useRef, useState } from 'react'

export type WebSocketHookResult = {
  latestMessage?: MessageEvent
  readyState: number
  sendMessage: (
    message: string | ArrayBufferLike | Blob | ArrayBufferView,
  ) => void
  disconnect: () => void
  connect: () => void
}

export const useWebSocket = (url: string): WebSocketHookResult => {
  const socketRef = useRef<WebSocket | null>(null)
  const reconnectCountRef = useRef(0)
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const shouldReconnectRef = useRef(true)
  const [latestMessage, setLatestMessage] = useState<MessageEvent>()
  const [readyState, setReadyState] = useState<number>(WebSocket.CLOSED)

  const clearReconnectTimer = useCallback(() => {
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current)
      reconnectTimerRef.current = null
    }
  }, [])

  const disconnect = useCallback(() => {
    shouldReconnectRef.current = false
    clearReconnectTimer()
    socketRef.current?.close()
    socketRef.current = null
    setReadyState(WebSocket.CLOSED)
  }, [clearReconnectTimer])

  const connect = useCallback(() => {
    if (!url || socketRef.current) {
      return
    }

    shouldReconnectRef.current = true
    clearReconnectTimer()

    const socket = new WebSocket(url)
    socketRef.current = socket
    setReadyState(socket.readyState)

    socket.onopen = () => {
      reconnectCountRef.current = 0
      setReadyState(socket.readyState)
    }
    socket.onclose = () => {
      if (socketRef.current === socket) {
        socketRef.current = null
      }

      setReadyState(socket.readyState)

      if (shouldReconnectRef.current && reconnectCountRef.current < 3) {
        reconnectCountRef.current += 1
        reconnectTimerRef.current = setTimeout(connect, 3000)
      }
    }
    socket.onerror = () => setReadyState(socket.readyState)
    socket.onmessage = (event) => setLatestMessage(event)
  }, [clearReconnectTimer, url])

  const sendMessage = useCallback<WebSocketHookResult['sendMessage']>(
    (message) => {
      if (socketRef.current?.readyState === WebSocket.OPEN) {
        socketRef.current.send(message)
      }
    },
    [],
  )

  useEffect(() => {
    reconnectCountRef.current = 0
    connect()

    return () => {
      disconnect()
    }
  }, [connect, disconnect])

  return {
    latestMessage,
    readyState,
    sendMessage,
    disconnect,
    connect,
  }
}
