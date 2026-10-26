import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createElement, useEffect, useState, type ReactNode } from 'react'

export function createAppQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        refetchOnWindowFocus: false,
        retry: false,
      },
      mutations: {
        retry: false,
      },
    },
  })
}

type WorkerQueryProviderProps = {
  children: ReactNode
  /**
   * The authenticated session epoch the subtree belongs to.
   */
  epoch: number
}

/**
 * The query client of one authenticated session.
 *
 * The application root keeps a client for the login page and for the relay
 * host, which are not bound to a Worker identity. Everything behind the
 * Worker auth gate is private to one authenticated session, so it reads
 * through a client of its own: a new epoch means a new session, and a new
 * session means a new client.
 *
 * The epoch is the element key, so the previous session's subtree is
 * unmounted and its client is retired in the same commit that starts the new
 * one. A response that arrives late belongs to the retired client and cannot
 * reach the new session's cache or its views.
 */
export function WorkerQueryProvider({
  children,
  epoch,
}: WorkerQueryProviderProps) {
  return createElement(WorkerQueryScope, { children, key: epoch })
}

function WorkerQueryScope({ children }: { children: ReactNode }) {
  const [client] = useState(createAppQueryClient)

  useEffect(() => {
    return () => {
      // The retired client is emptied on the way out. The requests it holds
      // are not aborted at the transport — the generated client carries no
      // abort signal — so cancelling them here is about the cache, not about
      // the network.
      void client.cancelQueries().catch(() => undefined)
      client.clear()
    }
  }, [client])

  return createElement(QueryClientProvider, { client }, children)
}
