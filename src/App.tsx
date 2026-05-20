import { useState, useEffect } from 'react'
import LoginPage from './LoginPage.tsx'
import ChatPage from './ChatPage.tsx'

export default function App() {
  const [sessionId, setSessionIdRaw] = useState<string | null>(() => {
    return localStorage.getItem('hiem_session_id')
  })
  const [loading, setLoading] = useState(true)

  const onLogin = (id: string | null) => {
    if (id) {
      localStorage.setItem('hiem_session_id', id)
    } else {
      localStorage.removeItem('hiem_session_id')
    }
    setSessionIdRaw(id)
  }

  useEffect(() => {
    const checkAuth = async () => {
      const stored = localStorage.getItem('hiem_session_id')
      if (stored) {
        setSessionIdRaw(stored)
      }
      setLoading(false)
    }
    checkAuth()
  }, [])

  if (loading) {
    return <div style={{ padding: 20 }}>Loading...</div>
  }

  if (!sessionId) {
    return <LoginPage onLogin={onLogin} />
  }

  return <ChatPage sessionId={sessionId} onLogout={() => onLogin(null)} />
}