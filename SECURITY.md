# Security policy

lockmerge runs inside git on files that decide which code a project builds,
so a merge that silently writes the wrong lockfile is treated as a security
issue, not only a bug.

## Reporting a vulnerability

Please report privately through GitHub: open the repository's **Security**
tab and choose **Report a vulnerability**. Don't open a public issue.

Include the lockfiles involved (base, ours, theirs), the command you ran, and
what you expected. We'll acknowledge the report, keep you updated, and credit
you in the fix if you'd like.

## Supported versions

lockmerge has not been released yet. Once it is, fixes go into the latest
release.
