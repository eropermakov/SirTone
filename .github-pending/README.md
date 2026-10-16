# CI workflows waiting for activation

GitHub refuses to accept workflow files from a token without the `workflow` scope.
When the owner's token has it (github.com → Settings → Developer settings → Personal access tokens → add `workflow`),
move this folder back: `git mv .github-pending/workflows .github/workflows`.
