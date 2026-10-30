# Cache rate alerts

The cache rate alert watches the fold-aware cache rate of a continuous session
and notifies the operator when a conversation drops below the configured
threshold inside the detection window.

Two delivery channels work side by side and can be used together:

- **DingTalk** — a robot webhook with an optional signing secret.
- **SMTP** — an email sent through an SMTP server.

Each channel has its own cooldown. A conversation that was already alerted on
one channel is not alerted again on that channel before its cooldown expires,
while the other channel stays free to alert. A channel whose delivery failed
is retried on its own later; a failure on one channel never blocks the other
one.

## Configuring the policy

Open **Settings → Cache alerts** as an administrator. The policy holds the
detection window, the minimum number of turns, the cache rate threshold and
the cooldown, plus the settings of each channel. The changes are sent when
**Save** is pressed; editing a field never sends anything by itself.

The alert needs PostgreSQL storage mode. On a backend that cannot keep the
setting, the tab shows that the policy is unavailable instead of the form.

## SMTP

The SMTP channel needs:

- **SMTP server** — a host name or an address without a scheme.
- **Port** — 1 to 65535.
- **Transport security** — implicit TLS or STARTTLS. A plaintext SMTP
  transport is not offered.
- **Sender address** — the address the alert email is sent from.
- **Recipients** — one or more addresses; every recipient receives the alert.
- **Username** and **password** — optional. Leave both empty for a TLS relay
  that needs no authentication. A username and a password always go together.

The channel is off by default and can be switched off again without losing its
configuration.

### The password is write-only

The stored password is never sent back by the admin API, and the settings tab
shows only whether one is stored. A blank password field keeps the stored
password, so the field can be left empty on every save that does not change
the credential. Changing the username without typing a new password drops the
stored one, because a stored password belongs to the account it was issued
for.

### The password is stored in plain text

The SMTP password is stored in the database as written, in plain text, so that
the alert sender can use it. This is a deliberate choice of the deployment, and
it has a security consequence: anyone who can read the database, or a backup
of it, can read the password and can send mail as the configured sender.

To limit that exposure:

- Keep the database and its backups on storage that only the operators can
  read.
- Restrict the administrative accounts that may read the settings.
- Rotate the password at the SMTP server and in the settings together.
- Use a relay without authentication where the deployment allows it, so no
  password is stored at all.

## The first SMTP alert

A channel that is enabled for the first time has no cooldown record yet. The
first conversation that meets the threshold is therefore alerted right away,
without waiting for a cooldown from an earlier period. From then on the
channel follows the configured cooldown like the DingTalk channel does.
