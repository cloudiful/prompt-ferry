#!/usr/bin/env nu

export def dotenv-record [path: string = ".env"] {
  if not ($path | path exists) {
    return {}
  }

  let entries = (
    open $path
    | lines
    | each {|line|
        let trimmed = ($line | str trim)
        if ($trimmed == "" or ($trimmed | str starts-with "#")) {
          null
        } else {
          let parsed = ($trimmed | parse -r '^(?<key>[A-Za-z_][A-Za-z0-9_]*)=(?<value>.*)$')
          if ($parsed | is-empty) {
            null
          } else {
            let row = ($parsed | get 0)
            let value = ($row.value | str trim | str replace -r '^"(.*)"$' '$1' | str replace -r "^'(.*)'$" '$1')
            { key: $row.key, value: $value }
          }
        }
      }
    | compact
  )

  if ($entries | is-empty) {
    return {}
  }

  $entries | reduce -f {} {|item, acc| $acc | upsert $item.key $item.value }
}

export def resolve-string [value: string, env_name: string, fallback: string] {
  if (($value | str trim) != "") {
    $value
  } else {
    (($env | get -o $env_name) | default $fallback)
  }
}
