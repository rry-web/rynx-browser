# Security

rynx is a browser, so it reads hostile input all day: please report anything that lets a page,
a server or a file do more than it should.

## Reporting a vulnerability

Report it privately through GitHub: the repository's **Security** tab, **Report a
vulnerability**. Please don't open a public issue for it. Include what an attacker controls (a
page, a server, a file), what they can make rynx do, and how to reproduce it; a page or file that
shows it is ideal.

The maintainer will reply once the report has been looked at. Fixes go into the next release, which existing copies are
offered through the in-browser updater, and the advisory is published with it, crediting you
unless you'd rather not be.

## What counts

Anything that breaks one of the promises in the README's
[Security and privacy](README.md#security-and-privacy) section, for example:

- running code, reading or writing files outside the Downloads and settings folders, or getting
  past the sandbox;
- sending requests to the local network from a public page (links, forms, images, redirects);
- taking over the terminal with escape sequences, or crashing or freezing the browser with a page;
- tracking: anything that lets a site recognise you across visits;
- the updater installing something not signed with the release key;
- I2P traffic leaving the I2P proxy.

## Supported versions

Only the latest release is supported: fixes aren't backported, and the updater offers each new
release to existing copies.
