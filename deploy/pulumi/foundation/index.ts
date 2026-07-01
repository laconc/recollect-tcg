/**
 * Recollect — FOUNDATION (tech-design §10.1, the launch-infra split).
 *
 * Run ONCE, with short-lived ADMIN creds (AWS SSO — see deploy/README.md). This is the
 * account-level, rarely-changing scaffolding the per-release PLATFORM stack and CI depend on.
 * It is deliberately a SEPARATE Pulumi project (its own state, its own stack) from PLATFORM:
 * FOUNDATION is created once and seldom touched; PLATFORM churns every release. Splitting the
 * states means a routine `pulumi up` on PLATFORM can never accidentally replace the ECR repo or
 * the CI trust, and the blast radius of each stack is its own.
 *
 * Provisions, in one `pulumi up`:
 *   ECR            an `aws.ecr.Repository` for the server image — SCAN-ON-PUSH (CVE scan each
 *                  push) + IMMUTABLE tags (a pushed `sha-…` tag can't be overwritten) + a
 *                  LIFECYCLE POLICY that expires untagged churn and caps retained release images,
 *                  so the repo never grows without bound.
 *   GitHub OIDC    an `aws.iam.OpenIdConnectProvider` for token.actions.githubusercontent.com —
 *                  the federation that lets GitHub Actions mint SHORT-LIVED AWS creds with NO
 *                  stored secret (no access keys in repo secrets at all).
 *   CI role        a TIGHTLY-SCOPED `aws.iam.Role` CI assumes via that OIDC provider. Its TRUST
 *                  policy is pinned to THIS repo's MAIN branch (the `sub` claim
 *                  `repo:<owner/repo>:ref:refs/heads/main`), so only main-branch workflows of this
 *                  one repo can assume it. Its PERMISSIONS are ECR-PUSH ONLY: `ecr:GetAuthorizationToken`
 *                  (which AWS requires on `*`) + the Get/Batch/Put/Upload/Complete set on THIS repo's
 *                  ARN, and nothing else. It cannot read other repos, touch EC2/IAM, or deploy.
 *
 * Every resource is TAGGED via the AWS provider's defaultTags (Project/Environment/ManagedBy/Stack/
 * Repository) + a per-resource Name — see the `commonTags` block + deploy/README.md "Tagging".
 *
 * Outputs the ECR repo URL + the CI role ARN. CI consumes the role ARN (the `role-to-assume`); the
 * repo URL becomes PLATFORM's `serverImage = <repoUrl>:<gitRef>` (the box pulls, never builds).
 *
 * NOTHING deployment-unique (the GitHub org/repo, the region) is committed — those are operator
 * config, supplied at deploy time, so this repo stays GENERIC. No click-ops; reproducible from code.
 */
import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";

const cfg = new pulumi.Config();
// `region` defaults to us-east-2 (the maintainer's SSO-profile region). It's not deployment-unique —
// override it freely. Keep FOUNDATION and PLATFORM on the SAME region so the box pulls the image
// without cross-region data charges. Everything else that identifies a deployment is operator-supplied.
const region = (cfg.get("region") ?? "us-east-2") as aws.Region;
// REQUIRED — the GitHub "owner/repo" whose main-branch workflows may assume the CI role. Pinned in
// the trust policy's `sub` (below); deployment-unique, so no committed default.
const githubRepo = cfg.require("githubRepo");
// The ECR repository name for the server image (e.g. recollect-server). PLATFORM's serverImage is
// `<repoUrl>:<gitRef>`; CI pushes `<repoUrl>:<sha>` + `:latest`.
const imageName = cfg.get("imageName") ?? "recollect-server";
// Lifecycle knobs: expire untagged churn after N days; keep at most M sha-tagged release images.
const expireUntaggedAfterDays = cfg.getNumber("expireUntaggedAfterDays") ?? 14;
const keepReleaseImages = cfg.getNumber("keepReleaseImages") ?? 20;
// OPTIONAL — a TLS thumbprint for GitHub's OIDC endpoint. Modern AWS verifies GitHub's cert against
// the trust store and IGNORES this list, so it stays EMPTY and the provider is created without one.
const githubOidcThumbprint = cfg.get("githubOidcThumbprint") ?? "";
// The deployment ENVIRONMENT this stack stands up (e.g. production, staging). Tags EVERY resource via
// the provider's defaultTags below, so the console / Cost Explorer can slice by environment. Defaults
// to "production" — this launch infra IS the production account; override with
// `pulumi config set environment <name>` for a non-prod copy.
const environment = cfg.get("environment") ?? "production";

// ---------------------------------------------------------------------------------------------
// Tagging — one common set on EVERY resource via the AWS provider's defaultTags (no per-resource
// tagging needed; AWS applies these to every taggable resource the provider creates). Provenance +
// an IaC-ownership signal so a human in the console knows what owns a resource and never hand-edits it:
//   Project     constant — the product these belong to.
//   Environment the `environment` config (production by default).
//   ManagedBy   constant `pulumi` — this is IaC; don't mutate it in the console.
//   Stack       this Pulumi project's name (`foundation` here) — which half of the deploy owns it.
//   Repository  the source repo — REUSES githubRepo (deployment-unique → config-driven, never hardcoded).
// The per-resource `Name` tag (the console's display name) CANNOT come from defaultTags — it is added
// explicitly on each key resource below.
const commonTags: Record<string, string> = {
  Project: "recollect",
  Environment: environment,
  ManagedBy: "pulumi",
  Stack: pulumi.getProject(),
  Repository: githubRepo,
};

// Region-scoped AWS provider so a stack's region config actually places the ECR repo + role there.
// `defaultTags` makes every resource this provider creates inherit `commonTags`; a per-resource
// `tags` only needs to add what's resource-specific (e.g. `Name`) and AWS merges the two.
const awsProvider = new aws.Provider("aws", { region, defaultTags: { tags: commonTags } });
const awsOpts = { provider: awsProvider };

// ---------------------------------------------------------------------------------------------
// ECR — the server image registry the box pulls from (production no longer builds on-box).
// ---------------------------------------------------------------------------------------------

// The repository. SCAN-ON-PUSH runs a CVE scan on every push (free basic scanning). IMMUTABLE tags
// mean a pushed `sha-<commit>` (or `:latest`) tag can NEVER be overwritten to point at different
// bytes — a deployed image ref is reproducible, matching the best-practices "immutable tags" rule
// (the Helm chart already forbids `:latest`; here we make tags themselves immutable at the registry).
const repo = new aws.ecr.Repository(
  "server",
  {
    name: imageName,
    imageScanningConfiguration: { scanOnPush: true },
    // Tags are immutable EXCEPT `latest`. Release images are pinned by the
    // immutable `sha-<commit>` tag (what PLATFORM deploys), but CI also pushes a
    // moving `:latest` for convenience — which an all-immutable repo rejects on
    // every push after the first ("tag already exists and is immutable").
    // IMMUTABLE_WITH_EXCLUSION keeps `sha-*` immutable and lets `latest` be
    // overwritten. Updating mutability is an in-place change (no repo replace).
    imageTagMutability: "IMMUTABLE_WITH_EXCLUSION",
    imageTagMutabilityExclusionFilters: [
      { filterType: "WILDCARD", filter: "latest" },
    ],
    // A `pulumi destroy` of FOUNDATION should cleanly remove the repo even if images remain — this
    // stack is operator-managed scaffolding, not a data store; the images are rebuildable from git.
    forceDelete: true,
    // Project/Environment/ManagedBy/Stack/Repository come from the provider's defaultTags; only the
    // console display Name is per-resource.
    tags: { Name: "recollect-ecr" },
  },
  awsOpts,
);

// Lifecycle policy: bound the repo's storage. Rule 1 caps retained sha-tagged RELEASE images (older
// ones expire — generous enough to roll back several releases); rule 2 sweeps untagged churn (the
// dangling layers a re-push leaves behind) after a short window. ECR evaluates rules low→high
// priority; the `tagPrefixList` rule must precede the broad untagged rule.
new aws.ecr.LifecyclePolicy(
  "server",
  {
    repository: repo.name,
    policy: JSON.stringify({
      rules: [
        {
          rulePriority: 1,
          description: `Keep the ${keepReleaseImages} most recent sha-tagged release images.`,
          selection: {
            tagStatus: "tagged",
            tagPrefixList: ["sha-"],
            countType: "imageCountMoreThan",
            countNumber: keepReleaseImages,
          },
          action: { type: "expire" },
        },
        {
          rulePriority: 2,
          description: `Expire untagged images older than ${expireUntaggedAfterDays} days.`,
          selection: {
            tagStatus: "untagged",
            countType: "sinceImagePushed",
            countUnit: "days",
            countNumber: expireUntaggedAfterDays,
          },
          action: { type: "expire" },
        },
      ],
    }),
  },
  awsOpts,
);

// ---------------------------------------------------------------------------------------------
// GitHub Actions OIDC — keyless federation (no stored AWS access keys anywhere).
// ---------------------------------------------------------------------------------------------

// The OIDC identity provider for GitHub Actions. GitHub mints a short-lived OIDC token per job;
// AWS STS trades it for short-lived role creds when the token's claims match the role's trust
// policy (below). `clientIdLists` is the audience GitHub must request — `sts.amazonaws.com` is the
// value the official aws-actions/configure-aws-credentials uses by default.
//
// `thumbprintLists`: modern AWS validates GitHub's OIDC TLS cert against its own trust store and
// ignores this list, so we pass NONE unless an operator pins one (the rare account/region that
// still needs it). Passing `[]` is correct and current — see deploy/README.md.
const githubOidc = new aws.iam.OpenIdConnectProvider(
  "github-actions",
  {
    url: "https://token.actions.githubusercontent.com",
    clientIdLists: ["sts.amazonaws.com"],
    thumbprintLists: githubOidcThumbprint ? [githubOidcThumbprint] : [],
    // common tags via defaultTags; only the console display Name is per-resource.
    tags: { Name: "recollect-github-oidc" },
  },
  awsOpts,
);

// ---------------------------------------------------------------------------------------------
// The CI push role — assumed via OIDC; ECR-push to THIS repo ONLY; trusted to THIS repo's main.
// ---------------------------------------------------------------------------------------------

// TRUST policy: allow `sts:AssumeRoleWithWebIdentity` from the GitHub OIDC provider ONLY when
//   - `aud` == sts.amazonaws.com (the audience the provider above accepts), AND
//   - `sub` == repo:<owner/repo>:ref:refs/heads/main — i.e. a workflow on THIS repo's MAIN branch.
// The `sub` is a StringLike with an EXACT value (no wildcard): a fork, a PR from a fork, another
// branch, a tag, or a different repo all present a different `sub` and are refused. This is the
// crux of the least-privilege story — the credential can only be minted by main-branch CI here.
const ciAssumeRolePolicy = pulumi
  .all([githubOidc.arn, githubRepo])
  .apply(([providerArn, repoSlug]) =>
    JSON.stringify({
      Version: "2012-10-17",
      Statement: [
        {
          Effect: "Allow",
          Principal: { Federated: providerArn },
          Action: "sts:AssumeRoleWithWebIdentity",
          Condition: {
            StringEquals: {
              "token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
            },
            StringLike: {
              "token.actions.githubusercontent.com:sub": `repo:${repoSlug}:ref:refs/heads/main`,
            },
          },
        },
      ],
    }),
  );

const ciRole = new aws.iam.Role(
  "ci-ecr-push",
  {
    name: "recollect-ci-ecr-push",
    description:
      "Assumed by GitHub Actions (this repo, main branch) via OIDC to push the server image to ECR. ECR-push only.",
    assumeRolePolicy: ciAssumeRolePolicy,
    // Cap a misused session: a build that somehow ran long can't hold creds indefinitely. The push
    // is minutes; an hour is ample headroom while bounding exposure.
    maxSessionDuration: 3600,
    // common tags via defaultTags; only the console display Name is per-resource.
    tags: { Name: "recollect-ci-role" },
  },
  awsOpts,
);

// PERMISSIONS: ECR-push to THIS repo's ARN, and nothing else. Two statements because
// `ecr:GetAuthorizationToken` (the `docker login` step) is account-level and AWS requires it on
// `Resource: *` — it grants only a short-lived registry auth token, not access to any repo's
// contents. Every CONTENT action (pull-during-build cache reads + the push: layer existence checks,
// blob uploads, manifest puts) is scoped to the ONE repo ARN. No delete, no other repo, no `*` on
// content. This is exactly the push surface and not one action more.
new aws.iam.RolePolicy(
  "ci-ecr-push",
  {
    role: ciRole.id,
    policy: repo.arn.apply((repoArn) =>
      JSON.stringify({
        Version: "2012-10-17",
        Statement: [
          {
            Sid: "EcrAuthToken",
            Effect: "Allow",
            Action: "ecr:GetAuthorizationToken",
            Resource: "*",
          },
          {
            Sid: "EcrPushThisRepoOnly",
            Effect: "Allow",
            Action: [
              // Read side (BuildKit's `cache-from`/layer reuse + `docker login` repo checks):
              "ecr:BatchCheckLayerAvailability",
              "ecr:BatchGetImage",
              "ecr:GetDownloadUrlForLayer",
              // Push side (upload layers, then put the manifest under the sha + latest tags):
              "ecr:InitiateLayerUpload",
              "ecr:UploadLayerPart",
              "ecr:CompleteLayerUpload",
              "ecr:PutImage",
            ],
            Resource: repoArn,
          },
        ],
      }),
    ),
  },
  awsOpts,
);

// ---------------------------------------------------------------------------------------------
// Outputs — what CI and PLATFORM consume after `pulumi up`.
// ---------------------------------------------------------------------------------------------

// The ECR repository URL (…dkr.ecr.<region>.amazonaws.com/<imageName>). CI pushes `${repoUrl}:<sha>`
// + `${repoUrl}:latest`; PLATFORM sets `serverImage = ${repoUrl}:<gitRef>` so the box PULLS it.
export const repoUrl = repo.repositoryUrl;
// The repository ARN (for reference / cross-account grants if ever needed).
export const repoArn = repo.arn;
// The CI role ARN — CI's `role-to-assume` (configure-aws-credentials). Surface it so the operator
// can drop it into the repo's `AWS_ROLE_ARN` variable: `pulumi stack output ciRoleArn`.
export const ciRoleArn = ciRole.arn;
// The OIDC provider ARN (informational — the trust references it).
export const githubOidcProviderArn = githubOidc.arn;
// The region the foundation lives in — handy when wiring CI / PLATFORM to match.
export const ecrRegion = region;
