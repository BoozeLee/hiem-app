import { useState } from 'react'

interface ChainInspectorProps {
  sessionId: string
}

interface ToolCall {
  name: string
  arguments: Record<string, unknown>
  result?: string
}

interface AgentStep {
  step: string
  input?: string
  output?: string
  toolCalls?: ToolCall[]
  latencyMs?: number
}

export default function ChainInspector({ sessionId }: ChainInspectorProps) {
  const [isOpen, setIsOpen] = useState(false)
  const [steps, setSteps] = useState<AgentStep[]>([])
  const [isLoading, setIsLoading] = useState(false)

  const inspectSession = async () => {
    setIsLoading(true)
    try {
      const response = await fetch(`/api/agent/session/${sessionId}`)
      if (response.ok) {
        const data = await response.json()
        setSteps(data.steps || [])
      }
    } catch (e) {
      console.error('Failed to inspect session:', e)
    } finally {
      setIsLoading(false)
    }
  }

  if (!isOpen) {
    return (
      <button
        onClick={() => {
          setIsOpen(true)
          inspectSession()
        }}
        style={{
          background: 'transparent',
          border: '1px solid #30363d',
          color: '#8b949e',
          padding: '6px 12px',
          borderRadius: 6,
          fontSize: 12,
          cursor: 'pointer',
        }}
      >
        Inspector
      </button>
    )
  }

  return (
    <div style={{
      position: 'absolute',
      right: 20,
      top: 60,
      width: 320,
      maxHeight: 'calc(100vh - 100px)',
      background: '#0d1117',
      border: '1px solid #30363d',
      borderRadius: 8,
      padding: 12,
      zIndex: 100,
      overflowY: 'auto',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
        <span style={{ fontWeight: 600, fontSize: 14, color: '#e6edf3' }}>Agent Inspector</span>
        <button
          onClick={() => setIsOpen(false)}
          style={{
            background: 'transparent',
            border: 'none',
            color: '#8b949e',
            cursor: 'pointer',
            fontSize: 16,
          }}
        >
          ×
        </button>
      </div>

      {isLoading ? (
        <div style={{ color: '#8b949e', fontSize: 13 }}>Loading...</div>
      ) : steps.length === 0 ? (
        <div style={{ color: '#484f58', fontSize: 12 }}>No steps recorded yet.</div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          {steps.map((step, i) => (
            <div key={i} style={{
              background: '#161b22',
              border: '1px solid #30363d',
              borderRadius: 6,
              padding: 8,
            }}>
              <div style={{ fontSize: 12, color: '#58a6ff', fontWeight: 600, marginBottom: 4 }}>
                {step.step}
                {step.latencyMs && <span style={{ float: 'right', color: '#484f58' }}>{step.latencyMs}ms</span>}
              </div>
              {step.toolCalls && step.toolCalls.length > 0 && (
                <div style={{ fontSize: 11, color: '#e6edf3' }}>
                  {step.toolCalls.map((tc, j) => (
                    <div key={j} style={{ marginBottom: 4 }}>
                      <span style={{ color: '#79c0ff' }}>{tc.name}</span>
                      {tc.arguments && Object.keys(tc.arguments).length > 0 && (
                        <div style={{ color: '#484f58', marginLeft: 8 }}>
                          {JSON.stringify(tc.arguments)}
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}