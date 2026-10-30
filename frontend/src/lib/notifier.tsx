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
import { X } from 'lucide-react'
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

function NotificationItem({
  item,
  dismiss,
}: {
  item: Notification
  dismiss: (id: number) => void
}) {
  useEffect(() => {
    const timer = window.setTimeout(() => dismiss(item.id), 5000)
    return () => window.clearTimeout(timer)
  }, [dismiss, item.id])

  return (
    <div
      role={item.kind === 'error' ? 'alert' : 'status'}
      className="pointer-events-auto flex items-start gap-2 rounded-md border bg-card px-4 py-3 text-sm text-card-foreground shadow-lg"
    >
      <span className="min-w-0 flex-1 break-words">{item.message}</span>
      <button
        aria-label="Close"
        className="shrink-0 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        onClick={() => dismiss(item.id)}
        type="button"
      >
        <X aria-hidden="true" className="size-4" />
      </button>
    </div>
  )
}

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
        className="pointer-events-none fixed right-4 bottom-4 z-[70] grid w-[min(24rem,calc(100vw-2rem))] gap-2"
      >
        {notifications.map((item) => (
          <NotificationItem key={item.id} item={item} dismiss={dismiss} />
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
