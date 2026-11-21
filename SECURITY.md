# Security policy

## Reporting
Please report vulnerabilities privately through the repository's security advisory feature
("Report a vulnerability"), not in public issues. Include the version
(`3dmousectl version`), your distribution and desktop, and reproduction steps. Expect an
acknowledgement within 7 days.

## Scope and design
3dmouse-linux runs one root service (with no capabilities) that can inject keyboard and mouse
input, so these are in scope:
- privilege escalation or unauthorised control through `/run/3dmouse/broker.sock` or
  `/run/spnav.sock`
- input injection while the session is locked, or by a user who isn't the active seat user
- a profile import that leads to command execution, or to Alt+SysRq or other dangerous key output
- malformed HID reports causing crashes or memory growth
- information leaking into support bundles (serial numbers, window titles, keystrokes)

Mitigations are described in `docs/adr/0002-broker-agent-split.md`,
`docs/adr/0005-service-permissions.md` and `docs/research/risk-register.md`.

## Known limitations (not vulnerabilities, but disclosed)
- Lock detection is polled every 1 s. Desktops that give no lock signal are not protected.
- uinput input cannot be told apart from real input by applications.
