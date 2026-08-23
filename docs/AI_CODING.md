# AI Coding Process and Provenance

AI coding assistance was used as an engineering collaborator for planning,
implementation, review, test design, documentation, and validation. Human
review remains responsible for accepting changes, running final checks, and
submitting the resulting package.

The source repository and its commit history are the authoritative record of
accepted code. Generated binaries and evaluation artifacts are not treated as
source. Deterministic tests, linting, ASCII checks, dependency inspection, and
package verification provide reproducible evidence for reviewed changes.

The complete exported AI conversation logs are user-supplied packaging inputs.
They must be placed in the explicit logs directory passed to
`scripts/submission_package.py create`; the creator rejects a missing or empty
directory. The package copies those exports under `submit_pkg/logs/` without
inventing, summarizing, or replacing them. No fake log is checked into the
repository. CI may create a clearly labeled synthetic text log solely to test
the TAR structure, and that CI archive is not a final submission.
