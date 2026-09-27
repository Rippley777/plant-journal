# Docker CI/CD on GitHub

[Docker CI/CD](../.github/workflows/docker.yml) uses the existing multistage
Dockerfile to build a `linux/amd64` image. It runs on pull requests targeting
`main`, pushes to `main`, and manual workflow dispatches.

- Every run builds the image (including `cargo test --locked` in the Dockerfile)
  and runs `--version` in the runtime container with networking disabled.
- Pull requests, including forks, do not log in to Azure or publish images.
- Successful `main` runs publish the **same tested image**, transferred between
  jobs as a one-day artifact, to
  `plantjournaldaac2bd8.azurecr.io/plant-journal:sha-<full-Git-commit-SHA>`.
- Manual runs on other branches build/test only. No moving `latest` tag is updated.
- Buildx caches build layers between runs. Dependencies are pinned in `Cargo.lock`;
  GitHub actions are pinned to verified release commits with version comments.
- The workflow does not run SQL migrations, access `.env`, or deploy an App Service
  website. It handles continuous integration and delivery to the registry.

## One-time GitHub and Azure setup

This checkout currently has no GitHub remote configured. Push it to your intended
GitHub repository, including the workflow and Docker build inputs. Pull-request CI
works without Azure identity settings; publishing requires the setup below.

1. Create a GitHub environment named **`acr-publish`** under repository Settings →
   Environments. Restrict its deployment branches to **`main`**.
2. Create or choose a dedicated Microsoft Entra application/service principal or
   user-assigned managed identity for GitHub publishing. Give it **AcrPush** on
   the `plantjournaldaac2bd8` registry scope, rather than subscription-wide access.
   If the registry uses ABAC repository permissions, use the corresponding
   Container Registry Repository Writer role for `plant-journal` instead.
3. Add a federated credential to that identity for GitHub Actions:

   | Field | Value |
   | --- | --- |
   | Issuer | `https://token.actions.githubusercontent.com` |
   | Audience | `api://AzureADTokenExchange` |
   | Entity type | Environment |
   | GitHub environment | `acr-publish` |
   | Subject (standard GitHub format) | `repo:OWNER/REPOSITORY:environment:acr-publish` |

   Substitute your actual GitHub owner and repository, matching case. If your
   organization customizes OIDC subjects, configure Azure to match the actual
   subject template. A branch-based subject will not match this environment job.

4. Add these GitHub Actions secrets, either on the repository or its `acr-publish`
   environment:

   | Secret | Value |
   | --- | --- |
   | `AZURE_CLIENT_ID` | Client/application ID of the publishing identity |
   | `AZURE_TENANT_ID` | Microsoft Entra directory ID |
   | `AZURE_SUBSCRIPTION_ID` | Subscription containing the registry |

   GitHub exchanges an OIDC token for Azure access; no Azure client secret,
   registry administrator password, or SQL password is needed.

5. Open Actions → Docker CI/CD → Run workflow and select `main`, or push a commit
   to `main`. The publish job summary gives the resulting image reference. Docker
   push logs also include its registry digest.

See [GitHub's Azure OIDC setup](https://docs.github.com/en/actions/how-tos/secure-your-work/security-harden-deployments/oidc-in-azure)
and the [Azure Login action](https://github.com/Azure/login) for identity setup.
Container/runtime settings remain in [azure-container.md](azure-container.md).

If you rename the main branch, update the workflow's branch filters and publish
conditions, plus the GitHub environment's deployment branch rule. If you change
registries, update `ACR_NAME` and `ACR_LOGIN_SERVER` in the workflow and scope the
publishing identity to the new registry.
