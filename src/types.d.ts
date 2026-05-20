declare global {
  interface Window {
    __TAURI__: {
      invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
    }
  }
}

export {}
