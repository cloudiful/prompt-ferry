#!/usr/bin/env nu

def normalize-base-url [base_url: string] {
  let trimmed = ($base_url | str trim --right --char "/")
  if ($trimmed | str ends-with "/v1") {
    $trimmed
  } else {
    $"($trimmed)/v1"
  }
}

def extract-output-text [payload: any] {
  let direct = ($payload | get -o output_text | default "")
  if (($direct | str length) > 0) {
    return $direct
  }

  let nested = (
    $payload
    | get -o output
    | default []
    | where {|item| (($item | get -o type | default "") == "message")}
    | each {|item|
        $item
        | get -o content
        | default []
        | where {|part| (($part | get -o type | default "") == "output_text")}
        | each {|part| $part | get -o text | default "" }
      }
    | flatten
  )

  $nested | str join ""
}

def extract-stream-events [body: string] {
  $body
  | lines
  | where {|line| ($line | str starts-with "data:")}
  | each {|line|
      let data = ($line | str replace -r '^data:\s*' "")
      if $data == "[DONE]" {
        {type: "[DONE]"}
      } else {
        $data | from json
      }
    }
}

def main [
  --base-url (-b): string = "http://127.0.0.1:8787"
  --api-key (-k): string
  --model (-m): string = "deepseek-chat"
  --prompt (-p): string = ""
  --expected (-e): string = ""
  --max-output-tokens (-x): int = 256
  --stream (-s)
  --timeout-sec (-t): int = 120
  --insecure (-i)
] {
  let url = $"(normalize-base-url $base_url)/responses"
  let marker = if $expected != "" {
    $expected
  } else {
    let short_id = ((random uuid) | split row "-" | get 0)
    $"BRIDGE_OK_($short_id)"
  }
  let final_prompt = if $prompt != "" {
    $prompt
  } else {
    $"Return exactly this ASCII text, no quotes, no extra text: ($marker)"
  }
  let payload = {
    model: $model
    stream: $stream
    max_output_tokens: $max_output_tokens
    input: [
      {
        role: "user"
        content: [
          {
            type: "input_text"
            text: $final_prompt
          }
        ]
      }
    ]
  }
  let payload_json = ($payload | to json -r)
  let body_file = (^mktemp | str trim)

  let curl_args = if $insecure {
    [
      --silent
      --show-error
      --location
      --max-time ($timeout_sec | into string)
      --request POST
      --url $url
      --header $"Authorization: Bearer ($api_key)"
      --header "Content-Type: application/json"
      --output $body_file
      --write-out "%{http_code}\n%{content_type}"
      --data $payload_json
      --insecure
    ]
  } else {
    [
      --silent
      --show-error
      --location
      --max-time ($timeout_sec | into string)
      --request POST
      --url $url
      --header $"Authorization: Bearer ($api_key)"
      --header "Content-Type: application/json"
      --output $body_file
      --write-out "%{http_code}\n%{content_type}"
      --data $payload_json
    ]
  }

  let result = (do { ^curl ...$curl_args } | complete)
  let body = (open --raw $body_file)
  rm -f $body_file

  if $result.exit_code != 0 {
    print $"curl failed with exit code ($result.exit_code)"
    if ($result.stderr | str length) > 0 {
      print $result.stderr
    }
    error make {msg: "request failed"}
  }

  let meta = ($result.stdout | lines)
  let status = (($meta | get 0 | default "0") | into int)
  let content_type = ($meta | get -o 1 | default "")

  print $"url: ($url)"
  print $"model: ($model)"
  print $"stream: ($stream)"
  print $"status: ($status)"
  print $"content-type: ($content_type)"
  print $"max_output_tokens: ($max_output_tokens)"
  print $"expected marker: ($marker)"

  if $status != 200 {
    print "response body:"
    print $body
    error make {msg: $"unexpected HTTP status: ($status)"}
  }

  if $stream {
    let events = (extract-stream-events $body)
    let event_types = (
      $events
      | each {|event| $event | get -o type | default "[unknown]"}
      | uniq
    )
    let delta_text = (
      $events
      | where {|event| (($event | get -o type | default "") == "response.output_text.delta")}
      | each {|event| $event | get -o delta | default "" }
      | str join ""
    )
    let completed = (
      $events
      | where {|event| (($event | get -o type | default "") == "response.completed")}
      | reverse
      | get -o 0
    )
    let completed_text = if ($completed | is-empty) {
      ""
    } else {
      extract-output-text ($completed | get response)
    }
    let final_text = if $completed_text != "" { $completed_text } else { $delta_text }

    print $"events: ($event_types | str join ', ')"
    print $"delta_text: ($delta_text)"
    print $"final_text: ($final_text)"

    if $final_text == "" {
      print "raw stream body:"
      print $body
      error make {msg: "stream finished but final text is empty"}
    }

    if not ($final_text | str contains $marker) {
      print "raw stream body:"
      print $body
      error make {msg: "stream text does not contain expected marker"}
    }

    print "PASS: stream response contains expected marker"
    return
  }

  let parsed = (try { $body | from json } catch { null })
  if ($parsed | is-empty) {
    print "response body:"
    print $body
    error make {msg: "response body is not valid JSON"}
  }

  let object = ($parsed | get -o object | default "")
  let output_count = ($parsed | get -o output | default [] | length)
  let output_text = (extract-output-text $parsed)
  let output_tokens = (
    $parsed
    | get -o usage
    | default {}
    | get -o output_tokens
    | default 0
  )

  print $"object: ($object)"
  print $"output_count: ($output_count)"
  print $"usage.output_tokens: ($output_tokens)"
  print $"output_text: ($output_text)"

  if $output_text == "" {
    print "full response JSON:"
    print ($parsed | to json)
    error make {msg: "response output_text is empty"}
  }

  if not ($output_text | str contains $marker) {
    let likely_truncated = (
      $output_text != ""
      and ($marker | str starts-with $output_text)
      and ($output_tokens >= $max_output_tokens)
    )
    print "full response JSON:"
    print ($parsed | to json)
    if $likely_truncated {
      error make {
        msg: "response output_text looks truncated by max_output_tokens; increase -x or use a shorter marker"
      }
    }
    error make {msg: "response output_text does not contain expected marker"}
  }

  print "PASS: non-stream response contains expected marker"
}
