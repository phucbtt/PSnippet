# Security policy

PSnippet installs a low-level keyboard hook and injects keystrokes/clipboard text, so security reports are taken seriously.

**Please do not open a public issue for vulnerabilities.** Use GitHub's *Report a vulnerability* (private security advisory) on this repository.
Include the Windows version, steps to reproduce and the impact. We aim to acknowledge within 7 days.

Design guarantees you can test against: no network access, no telemetry, no logging of typed text or message content, no auto-send,
password fields are refused, the clipboard is restored after pasting, no admin rights required.
