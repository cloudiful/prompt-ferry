import { createContext, useContext, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

export const HeaderActionsContext = createContext<HTMLElement | null>(null)

export function HeaderActions({ children }: { children: ReactNode }) {
  const target = useContext(HeaderActionsContext)
  return target ? createPortal(children, target) : children
}
