#!/usr/bin/env nu

use dev-env.nu *

def env-or-default [key: string, fallback: string] {
  (($env | get -o $key) | default $fallback)
}

def relay-is-local [relay_url: string] {
  let value = ($relay_url | str trim)
  (
    ($value | str starts-with "ws://127.0.0.1:")
    or ($value | str starts-with "wss://127.0.0.1:")
    or ($value | str starts-with "ws://localhost:")
    or ($value | str starts-with "wss://localhost:")
  )
}

def first-relay-url [relay_urls_text: string] {
  let value = ($relay_urls_text | str trim)
  if ($value | str starts-with "[") {
    let parsed = ($value | from json)
    if (($parsed | length) > 0) {
      return (($parsed | get 0) | into string)
    }
  }
  $value
}

def print-startup-summary [mode: string, relay_url: string] {
  print $"Starting local dev stack \(($mode)\)"
  if (relay-is-local $relay_url) {
    print "  relay               http://127.0.0.1:8787"
    print "  worker listener     http://127.0.0.1:8788"
  } else {
    print $"  relay               external \(($relay_url)\)"
  }
  print "  worker admin api    http://127.0.0.1:8789/api/v1"
  if $mode == "full" {
    print "  frontend            http://127.0.0.1:5171"
  }
  print "Stop with Ctrl+C"
}

def ensure-backend-binary [] {
  print "Building prompt-ferry once before startup..."
  ^cargo build
}

def ensure-frontend-deps [] {
  let node_modules = ("frontend" | path join "node_modules")
  if not ($node_modules | path exists) {
    print "Installing frontend dependencies with bun..."
    cd frontend
    ^bun install --no-save
  }
}

def default-env [] {
  {
    PROMPT_FERRY_LOGGING__LEVEL: (env-or-default "PROMPT_FERRY_LOGGING__LEVEL" "info")
    PROMPT_FERRY_DEV_DATABASE_URL: (env-or-default "PROMPT_FERRY_DEV_DATABASE_URL" (env-or-default "PROMPT_FERRY_WORKER__DATABASE_URL" ""))
    PROMPT_FERRY_RELAY__BIND: (env-or-default "PROMPT_FERRY_RELAY__BIND" "127.0.0.1:8787")
    PROMPT_FERRY_RELAY__WORKER_BIND: (env-or-default "PROMPT_FERRY_RELAY__WORKER_BIND" "127.0.0.1:8788")
    PROMPT_FERRY_RELAY__CLIENT_TOKEN: (env-or-default "PROMPT_FERRY_RELAY__CLIENT_TOKEN" "dev-client-token")
    PROMPT_FERRY_RELAY__WORKER_TOKEN: (env-or-default "PROMPT_FERRY_RELAY__WORKER_TOKEN" "dev-worker-token")
    PROMPT_FERRY_RELAY__REQUEST_TIMEOUT_SECONDS: (env-or-default "PROMPT_FERRY_RELAY__REQUEST_TIMEOUT_SECONDS" "300")
    PROMPT_FERRY_WORKER__RELAY_URLS: (env-or-default "PROMPT_FERRY_WORKER__RELAY_URLS" '["ws://127.0.0.1:8788/ws/worker"]')
    PROMPT_FERRY_WORKER__WORKER_TOKEN: (env-or-default "PROMPT_FERRY_WORKER__WORKER_TOKEN" "dev-worker-token")
    PROMPT_FERRY_WORKER__DATABASE_URL: (env-or-default "PROMPT_FERRY_WORKER__DATABASE_URL" (env-or-default "PROMPT_FERRY_DEV_DATABASE_URL" ""))
    PROMPT_FERRY_WORKER__ADMIN_BIND: (env-or-default "PROMPT_FERRY_WORKER__ADMIN_BIND" "127.0.0.1:8789")
    PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN: (env-or-default "PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN" "admin")
    PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD: (env-or-default "PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD" "change-me-now")
    PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY: (env-or-default "PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY" "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=")
  }
}

def ensure-required-env [] {
  let db_url = (($env | get -o PROMPT_FERRY_DEV_DATABASE_URL) | default (($env | get -o PROMPT_FERRY_WORKER__DATABASE_URL) | default ""))
  if (($db_url | str trim) == "") {
    error make {
      msg: "missing database URL"
      help: "Set PROMPT_FERRY_DEV_DATABASE_URL or PROMPT_FERRY_WORKER__DATABASE_URL in .env."
    }
  }
}

def run-stack [mode: string] {
  let dot = (dotenv-record)
  load-env $dot
  ensure-required-env
  ensure-backend-binary
  if $mode == "full" {
    ensure-frontend-deps
  }
  let effective = ($dot | merge (default-env))
  print-startup-summary $mode (first-relay-url $effective.PROMPT_FERRY_WORKER__RELAY_URLS)

  with-env $effective {
    run-external bash scripts/dev-supervisor.sh $mode
  }
}

def main [command?: string] {
  let action = ($command | default "help")

  match $action {
    "backend" => {
      run-stack "backend"
    }
    "full" => {
      run-stack "full"
    }
    "help" => {
      print "Usage: nu scripts/dev.nu <backend|full>"
      print "Commands:"
      print "  backend   Start relay and worker"
      print "  full      Start relay, worker, and frontend Vite dev server"
      print "Config:"
      print "  Reads root .env automatically and treats it as the source of truth"
      print "  PROMPT_FERRY_DEV_DATABASE_URL or PROMPT_FERRY_WORKER__DATABASE_URL is required"
      print "  Switch local/remote settings by editing or commenting values in .env"
      print "  PROMPT_FERRY_LOGGING__LEVEL defaults to info"
      print "  managed dev defaults include a local relay secret master key if unset"
      print "  frontend deps auto-install once with bun if frontend/node_modules is missing"
    }
    _ => {
      error make {
        msg: $"unknown command: ($action)"
        help: "Run `nu scripts/dev.nu help` for usage."
      }
    }
  }
}
