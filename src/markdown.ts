/**
 * Lightweight inline markdown parser.
 * Supports: **bold**, `inline code`, [label](url)
 * No external dependencies.
 */

export function parseMarkdown(text: string): string {
  return text
    // Escape HTML first to prevent XSS
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    // Inline code (must come before bold/links to avoid conflicts)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    // Links: [label](url)
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>')
    // Bold: **text**
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
}

export function parseCodeBlocks(text: string): string {
  // Multi-line fenced code blocks ```...```
  return text.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, _lang, code) => {
    return `<pre style="background:#161b22;border-radius:6px;padding:12px;overflow-x:auto;margin:8px 0;"><code style="font-family:monospace;font-size:13px;color:#e6edf3;">${code.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>`
  })
}
