import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import './app/styles/index.css'
import App from './app/App.tsx'
import AppErrorBoundary from './app/components/AppErrorBoundary'
import { reportError } from './app/utils/errors'

const queryClient = new QueryClient()

window.addEventListener('error', (event) => {
  reportError(event.error ?? event.message, 'window.error')
})

window.addEventListener('unhandledrejection', (event) => {
  reportError(event.reason, 'window.unhandledrejection')
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </AppErrorBoundary>
  </StrictMode>,
)
