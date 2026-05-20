import { useState, useCallback, useEffect, useRef } from 'react'
import { parseMarkdown, parseCodeBlocks } from './markdown'
import {
  whoami,
  chat,
} from './tauri-api'
import ChainInspector from './ChainInspector'

interface GitHubMessageRow {
  id?: number
  full_name?: string
  number?: number
  title?: string
  state?: string
  author?: { login: string }
}

type Message = {
  role: 'user' | 'assistant'
  content: string
  timestamp: string
  status: 'sent' | 'streaming' | 'done'
  toolCall?: { name: string; output: string }
}

function isGitHubToolCall(content: string): { content: string; toolCall?: { name: string; output: string } } {
  const match = content.match(/\[TOOL:\s*(\w+)\]\s*\n?([\s\S]*)/)
  if (!match) return { content }
  const [, name, output] = match
  return { content: content.replace(match[0], '').trim(), toolCall: { name, output: output.trim() } }
}

function isGitHubJSON(jsonStr: string): boolean {
  try {
    const obj = JSON.parse(jsonStr)
    return Array.isArray(obj) && obj.length > 0 && typeof obj[0] === 'object'
      ? ('id' in obj[0] || 'full_name' in obj[0] || 'number' in obj[0] || 'title' in obj[0] || 'state' in obj[0] || 'author' in obj[0])
      : false
  } catch { return false }
}

function renderGitHubJSON(parsed: GitHubMessageRow[]): string {
  if (parsed.length === 0) return 'No results.'
  const keys = Object.keys(parsed[0])
  const header = keys.join(' | ')
  const rows = parsed.map((row: Record<string, unknown>) => keys.map(k => String(row[k] ?? '')).join(' | '))
  return `<pre style="text-align:left;">${[header, ...rows].join('\n')}</pre>`
}

export default function ChatPage({ sessionId, onLogout }: { sessionId: string; onLogout: () => void }) {
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState('')
  const [userLogin, setUserLogin] = useState('')
  const [userHasScrolledUp, setUserHasScrolledUp] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const chatEndRef = useRef<HTMLDivElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  const scrollToBottom = useCallback((behavior: ScrollBehavior = 'smooth') => {
    chatEndRef.current?.scrollIntoView({ behavior })
  }, [])

  useEffect(() => {
    if (!userHasScrolledUp) scrollToBottom()
  }, [messages, userHasScrolledUp, scrollToBottom])

  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const onScroll = () => {
      const { scrollTop, scrollHeight, clientHeight } = el
      setUserHasScrolledUp(scrollHeight - scrollTop - clientHeight > 100)
    }
    el.addEventListener('scroll', onScroll)
    return () => el.removeEventListener('scroll', onScroll)
  }, [])

  useEffect(() => {
    const fetchWhoami = async () => {
      try {
        const user = await whoami(sessionId)
        setUserLogin(user.login)
      } catch { /* ignore */ }
    }
    fetchWhoami()
  }, [sessionId])

  const getTime = () => new Date().toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })

  const sendMessage = useCallback(async () => {
    if (!input.trim() || isLoading) return

    const userMsg: Message = { role: 'user', content: input.trim(), timestamp: getTime(), status: 'sent' }
    const assistantMsg: Message = { role: 'assistant', content: '', timestamp: getTime(), status: 'streaming' }

    setMessages(m => [...m, userMsg, assistantMsg])
    setInput('')
    setIsLoading(true)
    scrollToBottom('instant')

     try {
        const response = await chat(sessionId, input.trim())
        setMessages(m => m.map(msg =>
          msg === assistantMsg
            ? { ...msg, content: response, status: 'done' }
            : msg
        ))
    } catch {
      setMessages(m => m.map(msg =>
        msg === assistantMsg
          ? { ...msg, content: 'Error: unable to reach the agent.', status: 'done' }
          : msg
      ))
    } finally {
      setIsLoading(false)
    }
  }, [input, sessionId, scrollToBottom, isLoading])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: '#0d1117' }}>
      <div style={{
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
        padding: '10px 20px', borderBottom: '1px solid #21262d', background: '#161b22'
      }}>
        <span style={{ fontWeight: 600, fontSize: 14 }}>HIEM{userLogin ? ` — ${userLogin}` : ''}</span>
        <button
          onClick={onLogout}
          style={{
            background: '#da3633', color: 'white', border: 'none',
            padding: '5px 14px', borderRadius: 6, fontSize: 12, fontWeight: 600
          }}
        >
          Logout
        </button>
      </div>

      <div
        ref={containerRef}
        style={{ flex: 1, overflowY: 'auto', padding: '16px 20px', display: 'flex', flexDirection: 'column', gap: 8 }}
      >
        {messages.map((msg, i) => {
          const isUser = msg.role === 'user'
          const { content: rawContent, toolCall } = isGitHubToolCall(msg.content)

          let rendered = parseCodeBlocks(parseMarkdown(rawContent))

          if (toolCall) {
            let toolOutput = toolCall.output
            const rawJsons = toolCall.output.match(/```json\n?([\s\S]*?)```/)
            if (rawJsons) {
              toolOutput = rawJsons[1].trim()
            }
            if (isGitHubJSON(toolOutput)) {
              toolOutput = renderGitHubJSON(JSON.parse(toolOutput))
            }
            rendered += `<details style="margin-top:6px;background:#161b22;border-radius:8px;border:1px solid #30363d;overflow:hidden">
              <summary style="padding:8px 12px;cursor:pointer;font-size:13px;color:#58a6ff;font-weight:600;display:flex;align-items:center;gap:6px;">
                ⚙️ GitHub Tool: ${toolCall.name}
              </summary>
              <pre style="margin:8px 12px;background:#0d1117;border-radius:6px;padding:10px;font-size:12px;font-family:monospace;color:#e6edf3;overflow-x:auto;white-space:pre-wrap;">${toolOutput.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</pre>
            </details>`
          }

          const showStreaming = !isUser && msg.status === 'streaming'

          return (
            <div key={i} style={{
              display: 'flex', flexDirection: 'column', maxWidth: '72%',
              alignSelf: isUser ? 'flex-end' : 'flex-start',
            }}>
              <div style={{
                padding: '10px 14px', borderRadius: 12,
                background: isUser ? '#1f6feb' : '#21262d',
                color: '#e6edf3', fontSize: 14, lineHeight: 1.5,
                borderBottomRightRadius: isUser ? 4 : 12,
                borderBottomLeftRadius: isUser ? 12 : 4,
              }}>
                <span dangerouslySetInnerHTML={{ __html: rendered }} />
                {showStreaming && <span> …</span>}
              </div>
              <span style={{
                fontSize: 11, color: '#484f58', marginTop: 2,
                marginLeft: isUser ? 'auto' : 4, marginRight: isUser ? 4 : 'auto',
                textAlign: 'left'
              }}>
                {msg.timestamp}
              </span>
            </div>
          )
        })}

        {isLoading && (
          <div style={{ display: 'flex', flexDirection: 'column', alignSelf: 'flex-start', maxWidth: '72%' }}>
            <div style={{
              padding: '10px 14px', borderRadius: 12,
              background: '#21262d', color: '#8b949e',
              fontSize: 14, borderBottomLeftRadius: 4,
            }}>
              <span className="thinking-dots">
                <span className="dot" /> <span className="dot" /> <span className="dot" />
              </span>
              {' '}Thinking...
            </div>
          </div>
        )}
        <div ref={chatEndRef} />
      </div>

      <div style={{
        display: 'flex', gap: 10, padding: '12px 16px',
        borderTop: '1px solid #21262d', background: '#161b22'
      }}>
        <ChainInspector sessionId={sessionId} />
        <input
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && !e.shiftKey && sendMessage()}
          placeholder="Ask about your repos..."
          style={{
            flex: 1, padding: '10px 14px', borderRadius: 8,
            border: '1px solid #30363d', background: '#0d1117', color: '#e6edf3',
            fontSize: 14, outline: 'none'
          }}
        />
        <button
          onClick={sendMessage}
          disabled={!input.trim() || isLoading}
          style={{
            padding: '10px 22px', borderRadius: 8,
            background: !input.trim() || isLoading ? '#21262d' : '#1f6feb',
            color: !input.trim() || isLoading ? '#484f58' : '#ffffff',
            border: 'none', fontWeight: 600, fontSize: 14, cursor: !input.trim() || isLoading ? 'default' : 'pointer',
          }}
        >
          {isLoading ? '' : 'Send'}
        </button>
      </div>

      <style>{`
        .thinking-dots .dot {
          display: inline-block;
          width: 6px; height: 6px;
          border-radius: 50%;
          background: #8b949e;
          animation: pulse 1.2s ease-in-out infinite;
        }
        .thinking-dots .dot:nth-child(2) { animation-delay: 0.2s; }
        .thinking-dots .dot:nth-child(3) { animation-delay: 0.4s; }
        @keyframes pulse {
          0%, 80%, 100% { opacity: 0.3; transform: scale(0.8); }
          40% { opacity: 1; transform: scale(1.1); }
        }
      `}</style>
    </div>
  )
}
