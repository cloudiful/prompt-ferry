import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { formatApiError } from '@/api'

type Notification = {
  id: number
  kind: 'success' | 'error'
  message: string
}

type NotifierValue = {
  notifySuccess: (message: string) => void
  notifyError: (message: string) => void
  notifyApiError: (cause: unknown) => void
}

const NotifierContext = createContext<NotifierValue | null>(null)

export function NotifierProvider({ children }: { children: ReactNode }) {
  const [notifications, setNotifications] = useState<Notification[]>([])
  const nextId = useRef(0)

  const publish = useCallback((kind: Notification['kind'], message: string) => {
    const id = ++nextId.current
    setNotifications((current) => [...current, { id, kind, message }])
  }, [])

  const notifySuccess = useCallback(
    (message: string) => publish('success', message),
    [publish],
  )
  const notifyError = useCallback(
    (message: string) => publish('error', message),
    [publish],
  )
  const notifyApiError = useCallback(
    (cause: unknown) => notifyError(formatApiError(cause)),
    [notifyError],
  )
  const dismiss = useCallback((id: number) => {
    setNotifications((current) => current.filter((item) => item.id !== id))
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined' || notifications.length === 0) return
    const timers = notifications.map((item) =>
      window.setTimeout(() => dismiss(item.id), 5000),
    )
    return () => timers.forEach((timer) => window.clearTimeout(timer))
  }, [dismiss, notifications])

  const value = useMemo(
    () => ({ notifySuccess, notifyError, notifyApiError }),
    [notifyApiError, notifyError, notifySuccess],
  )

  return (
    <NotifierContext.Provider value={value}>
      {children}
      <div
        aria-live="polite"
        aria-relevant="additions"
        className="fixed right-4 top-4 z-[70] grid w-[min(24rem,calc(100vw-2rem))] gap-2"
      >
        {notifications.map((item) => (
          <div
            key={item.id}
            role={item.kind === 'error' ? 'alert' : 'status'}
            className="rounded-md border bg-card px-4 py-3 text-sm text-card-foreground shadow-lg"
          >
            {item.message}
          </div>
        ))}
      </div>
    </NotifierContext.Provider>
  )
}

export function useNotifier(): NotifierValue {
  const value = useContext(NotifierContext)
  if (!value)
    throw new Error('useNotifier must be used within NotifierProvider')
  return value
}
