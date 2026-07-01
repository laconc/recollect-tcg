// Recollect — FOUNDATION (tech-design §10.1, the launch-infra split).
//
// Run ONCE, with short-lived ADMIN creds (AWS SSO — see deploy/README.md). This is the
// account-level, rarely-changing scaffolding the per-release PLATFORM stack and CI depend on.
// It is deliberately a SEPARATE Pulumi project (its own state, its own stack) from PLATFORM:
// FOUNDATION is created once and seldom touched; PLATFORM churns every release. Splitting the
// states means a routine `pulumi up` on PLATFORM can never accidentally replace the ECR repo or
// the CI trust, and the blast radius of each stack is its own.
//
// Provisions, in one `pulumi up`:
//
//	ECR          an ecr.Repository for the server image — SCAN-ON-PUSH (CVE scan each push) +
//	             IMMUTABLE tags (a pushed commit-SHA tag can't be overwritten; `latest` excepted) + a
//	             LIFECYCLE POLICY that expires untagged churn and caps retained release images.
//	GitHub OIDC  an iam.OpenIdConnectProvider for token.actions.githubusercontent.com — keyless
//	             federation so GitHub Actions mints SHORT-LIVED AWS creds with NO stored secret.
//	CI role      a tightly-scoped iam.Role CI assumes via that OIDC provider. Trust pinned to THIS
//	             repo's MAIN branch; permissions ECR-PUSH to THIS repo, plus ssm:SendCommand of the
//	             `recollect-update` document to the box (the in-place CD rollout) — and nothing else.
//	SSM doc      a locked-down ssm.Document ("recollect-update") CI sends to the box to roll it to a
//	             new image in place (runs the on-box recollect-update helper; hex-SHA-constrained param).
//
// Every resource is TAGGED via the AWS provider's defaultTags (Project/Environment/ManagedBy/Stack/
// Repository) + a per-resource Name. Outputs the ECR repo URL + the CI role ARN (CI's role-to-assume;
// PLATFORM's serverImage = <repoUrl>:<gitRef> — the box pulls, never builds). NOTHING deployment-
// unique (the GitHub org/repo, the region) is committed — those are operator config, supplied at
// deploy time, so this repo stays GENERIC.
package main

import (
	"encoding/json"
	"fmt"
	"strconv"

	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/ecr"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/iam"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/ssm"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

func main() {
	pulumi.Run(func(ctx *pulumi.Context) error {
		cfg := config.New(ctx, "")

		// getInt reads an integer config, honoring an explicit value (INCLUDING 0) and falling back to
		// def only when the key is genuinely unset — matching the TS `cfg.getNumber(...) ?? def` (a plain
		// cfg.GetInt can't tell an unset key from an explicit 0).
		getInt := func(key string, def int) int {
			if s := cfg.Get(key); s != "" {
				if v, err := strconv.Atoi(s); err == nil {
					return v
				}
			}
			return def
		}

		// `region` defaults to us-east-2 (the maintainer's SSO-profile region) — not deployment-unique,
		// override freely. Keep FOUNDATION and PLATFORM on the SAME region so the box pulls the image
		// without cross-region data charges.
		region := cfg.Get("region")
		if region == "" {
			region = "us-east-2"
		}
		// REQUIRED — the GitHub "owner/repo" whose main-branch workflows may assume the CI role. Pinned
		// in the trust policy's `sub` (below); deployment-unique, so no committed default.
		githubRepo := cfg.Require("githubRepo")
		// The ECR repository name for the server image (e.g. recollect-server). PLATFORM's serverImage
		// is `<repoUrl>:<gitRef>`; CI pushes `<repoUrl>:<sha>` + `:latest`.
		imageName := cfg.Get("imageName")
		if imageName == "" {
			imageName = "recollect-server"
		}
		// Lifecycle knobs: expire untagged churn after N days; keep at most M sha-tagged release images.
		expireUntaggedAfterDays := getInt("expireUntaggedAfterDays", 14)
		keepReleaseImages := getInt("keepReleaseImages", 20)
		// OPTIONAL — a TLS thumbprint for GitHub's OIDC endpoint. Modern AWS verifies GitHub's cert
		// against the trust store and IGNORES this list, so it stays EMPTY and the provider is created
		// without one.
		githubOidcThumbprint := cfg.Get("githubOidcThumbprint")
		// The deployment ENVIRONMENT this stack stands up (production by default); tags every resource
		// via the provider's defaultTags so the console / Cost Explorer can slice by environment.
		environment := cfg.Get("environment")
		if environment == "" {
			environment = "production"
		}

		// Tagging — one common set on EVERY resource via the AWS provider's defaultTags. Provenance +
		// an IaC-ownership signal so a human in the console knows what owns a resource and never hand-
		// edits it. The per-resource `Name` tag CANNOT come from defaultTags — it is added on each key
		// resource below.
		commonTags := pulumi.StringMap{
			"Project":     pulumi.String("recollect"),
			"Environment": pulumi.String(environment),
			"ManagedBy":   pulumi.String("pulumi"),
			"Stack":       pulumi.String(ctx.Project()),
			"Repository":  pulumi.String(githubRepo),
		}

		// Region-scoped AWS provider so a stack's region config actually places the ECR repo + role
		// there. `defaultTags` makes every resource this provider creates inherit `commonTags`; a
		// per-resource `tags` only needs to add what's resource-specific (e.g. `Name`).
		awsProvider, err := aws.NewProvider(ctx, "aws", &aws.ProviderArgs{
			Region:      pulumi.String(region),
			DefaultTags: &aws.ProviderDefaultTagsArgs{Tags: commonTags},
		})
		if err != nil {
			return err
		}
		awsProviderOpt := pulumi.Provider(awsProvider)

		// -----------------------------------------------------------------------------------------
		// ECR — the server image registry the box pulls from (production no longer builds on-box).
		// -----------------------------------------------------------------------------------------

		// The repository. SCAN-ON-PUSH runs a CVE scan on every push (free basic scanning). Tags are
		// immutable EXCEPT `latest` (IMMUTABLE_WITH_EXCLUSION): a pushed `sha-<commit>` tag can NEVER
		// be overwritten to point at different bytes (a deployed ref is reproducible), while the moving
		// `:latest` CI also pushes is allowed to be overwritten. forceDelete lets a `pulumi destroy`
		// cleanly remove the repo even if images remain — this is operator-managed scaffolding, not a
		// data store; the images are rebuildable from git.
		repo, err := ecr.NewRepository(ctx, "server", &ecr.RepositoryArgs{
			Name: pulumi.String(imageName),
			ImageScanningConfiguration: &ecr.RepositoryImageScanningConfigurationArgs{
				ScanOnPush: pulumi.Bool(true),
			},
			ImageTagMutability: pulumi.String("IMMUTABLE_WITH_EXCLUSION"),
			ImageTagMutabilityExclusionFilters: ecr.RepositoryImageTagMutabilityExclusionFilterArray{
				&ecr.RepositoryImageTagMutabilityExclusionFilterArgs{
					FilterType: pulumi.String("WILDCARD"),
					Filter:     pulumi.String("latest"),
				},
			},
			ForceDelete: pulumi.Bool(true),
			Tags:        pulumi.StringMap{"Name": pulumi.String("recollect-ecr")},
		}, awsProviderOpt)
		if err != nil {
			return err
		}

		// Lifecycle policy: bound the repo's storage. Rule 1 keeps the N most-recent images — release
		// images are tagged with the bare commit SHA (plus a floating `latest` on the newest), which share
		// no tag prefix to filter on, so this is a count over ANY tag status; rule 2 sweeps untagged churn
		// (the dangling layers a re-push leaves behind). ECR evaluates rules low→high priority, so the
		// count rule precedes the broad untagged rule.
		lifecyclePolicy, err := json.Marshal(map[string]any{
			"rules": []any{
				map[string]any{
					"rulePriority": 1,
					"description":  fmt.Sprintf("Keep the %d most recent release images.", keepReleaseImages),
					"selection": map[string]any{
						"tagStatus":   "any",
						"countType":   "imageCountMoreThan",
						"countNumber": keepReleaseImages,
					},
					"action": map[string]any{"type": "expire"},
				},
				map[string]any{
					"rulePriority": 2,
					"description":  fmt.Sprintf("Expire untagged images older than %d days.", expireUntaggedAfterDays),
					"selection": map[string]any{
						"tagStatus":   "untagged",
						"countType":   "sinceImagePushed",
						"countUnit":   "days",
						"countNumber": expireUntaggedAfterDays,
					},
					"action": map[string]any{"type": "expire"},
				},
			},
		})
		if err != nil {
			return err
		}
		if _, err := ecr.NewLifecyclePolicy(ctx, "server", &ecr.LifecyclePolicyArgs{
			Repository: repo.Name,
			Policy:     pulumi.String(string(lifecyclePolicy)),
		}, awsProviderOpt); err != nil {
			return err
		}

		// -----------------------------------------------------------------------------------------
		// GitHub Actions OIDC — keyless federation (no stored AWS access keys anywhere).
		// -----------------------------------------------------------------------------------------

		// The OIDC identity provider for GitHub Actions. GitHub mints a short-lived OIDC token per job;
		// AWS STS trades it for short-lived role creds when the token's claims match the role's trust
		// policy (below). `clientIdLists` is the audience GitHub must request — `sts.amazonaws.com` is
		// what aws-actions/configure-aws-credentials uses by default. `thumbprintLists` is empty:
		// modern AWS validates GitHub's OIDC TLS cert against its own trust store and ignores it,
		// unless an operator pins one (rare) — see deploy/README.md.
		thumbprints := pulumi.StringArray{}
		if githubOidcThumbprint != "" {
			thumbprints = pulumi.StringArray{pulumi.String(githubOidcThumbprint)}
		}
		githubOidc, err := iam.NewOpenIdConnectProvider(ctx, "github-actions", &iam.OpenIdConnectProviderArgs{
			Url:             pulumi.String("https://token.actions.githubusercontent.com"),
			ClientIdLists:   pulumi.StringArray{pulumi.String("sts.amazonaws.com")},
			ThumbprintLists: thumbprints,
			Tags:            pulumi.StringMap{"Name": pulumi.String("recollect-github-oidc")},
			// AWS AUTO-MANAGES the thumbprint for the well-known GitHub endpoint (it populates/rotates
			// one regardless of what we send), so our empty `thumbprintLists` perpetually diffs against
			// the value AWS returns — a benign but noisy drift on every preview. Ignore changes to the
			// field: modern AWS validates GitHub's cert against its own trust store and does not rely on
			// this list, so leaving it to AWS is correct. (A create still applies a pinned thumbprint if
			// `githubOidcThumbprint` is configured; ignoreChanges only affects subsequent updates.)
		}, awsProviderOpt, pulumi.IgnoreChanges([]string{"thumbprintLists"}))
		if err != nil {
			return err
		}

		// -----------------------------------------------------------------------------------------
		// The CI push role — assumed via OIDC; ECR-push to THIS repo ONLY; trusted to THIS repo's main.
		// -----------------------------------------------------------------------------------------

		// TRUST policy: allow sts:AssumeRoleWithWebIdentity from the GitHub OIDC provider ONLY when
		// `aud` == sts.amazonaws.com AND `sub` == repo:<owner/repo>:ref:refs/heads/main — a workflow on
		// THIS repo's MAIN branch. The `sub` is an EXACT StringLike (no wildcard): a fork, a PR from a
		// fork, another branch, a tag, or a different repo all present a different `sub` and are refused.
		assumeRolePolicy := githubOidc.Arn.ApplyT(func(providerArn string) (string, error) {
			doc := map[string]any{
				"Version": "2012-10-17",
				"Statement": []any{
					map[string]any{
						"Effect":    "Allow",
						"Principal": map[string]any{"Federated": providerArn},
						"Action":    "sts:AssumeRoleWithWebIdentity",
						"Condition": map[string]any{
							"StringEquals": map[string]any{
								"token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
							},
							"StringLike": map[string]any{
								"token.actions.githubusercontent.com:sub": fmt.Sprintf("repo:%s:ref:refs/heads/main", githubRepo),
							},
						},
					},
				},
			}
			b, err := json.Marshal(doc)
			return string(b), err
		}).(pulumi.StringOutput)

		ciRole, err := iam.NewRole(ctx, "ci-ecr-push", &iam.RoleArgs{
			Name:               pulumi.String("recollect-ci-ecr-push"),
			Description:        pulumi.String("Assumed by GitHub Actions (this repo, main branch) via OIDC to push the server image to ECR. ECR-push only."),
			AssumeRolePolicy:   assumeRolePolicy,
			MaxSessionDuration: pulumi.Int(3600),
			Tags:               pulumi.StringMap{"Name": pulumi.String("recollect-ci-role")},
		}, awsProviderOpt)
		if err != nil {
			return err
		}

		// A locked-down SSM Command document CI invokes to roll the box to a new image IN PLACE (no box
		// replacement): it runs the on-box `recollect-update <GitRef>` helper, which re-pulls the image
		// tagged <GitRef> and recreates the server container. `GitRef` is constrained to a hex SHA
		// (allowedPattern), so a caller can't smuggle shell through the parameter — CI can run THIS one
		// action and nothing else on the box (the SendCommand grant below is scoped to this doc + the box).
		updateDoc, err := ssm.NewDocument(ctx, "recollect-update", &ssm.DocumentArgs{
			Name:           pulumi.String("recollect-update"),
			DocumentType:   pulumi.String("Command"),
			DocumentFormat: pulumi.String("YAML"),
			Content: pulumi.String(`schemaVersion: "2.2"
description: "Roll the recollect server to a git commit's image, in place (no box replacement)."
parameters:
  GitRef:
    type: String
    description: "The git commit SHA to deploy (also the image tag CI pushes)."
    allowedPattern: "^[0-9a-fA-F]{7,40}$"
mainSteps:
  - action: aws:runShellScript
    name: recollectUpdate
    inputs:
      runCommand:
        - "sudo /usr/local/bin/recollect-update '{{ GitRef }}'"
`),
			Tags: pulumi.StringMap{"Name": pulumi.String("recollect-update")},
		}, awsProviderOpt)
		if err != nil {
			return err
		}

		// PERMISSIONS: (1) ECR-push to THIS repo's ARN — `ecr:GetAuthorizationToken` (the `docker login`
		// step) is account-level so AWS requires it on Resource `*` (a short-lived registry token only);
		// every CONTENT action (cache reads + the push) is scoped to the ONE repo ARN. (2) SSM: send ONLY
		// the `recollect-update` document, ONLY to the box (instances tagged Name=recollect-server), plus
		// read the command result — the deploy-image.yml CD rollout. No delete, no other repo, no `*` on
		// content, no arbitrary SSM/shell.
		rolePolicy := pulumi.All(repo.Arn, updateDoc.Arn).ApplyT(func(args []any) (string, error) {
			repoArn := args[0].(string)
			docArn := args[1].(string)
			doc := map[string]any{
				"Version": "2012-10-17",
				"Statement": []any{
					map[string]any{
						"Sid":      "EcrAuthToken",
						"Effect":   "Allow",
						"Action":   "ecr:GetAuthorizationToken",
						"Resource": "*",
					},
					map[string]any{
						"Sid":    "EcrPushThisRepoOnly",
						"Effect": "Allow",
						"Action": []string{
							"ecr:BatchCheckLayerAvailability",
							"ecr:BatchGetImage",
							"ecr:GetDownloadUrlForLayer",
							"ecr:InitiateLayerUpload",
							"ecr:UploadLayerPart",
							"ecr:CompleteLayerUpload",
							"ecr:PutImage",
						},
						"Resource": repoArn,
					},
					map[string]any{
						"Sid":      "SsmSendToRecollectBoxOnly",
						"Effect":   "Allow",
						"Action":   "ssm:SendCommand",
						"Resource": "arn:aws:ec2:*:*:instance/*",
						"Condition": map[string]any{
							"StringEquals": map[string]any{"ssm:resourceTag/Name": "recollect-server"},
						},
					},
					map[string]any{
						"Sid":      "SsmSendRecollectUpdateDocOnly",
						"Effect":   "Allow",
						"Action":   "ssm:SendCommand",
						"Resource": docArn,
					},
					map[string]any{
						"Sid":      "SsmReadCommandResult",
						"Effect":   "Allow",
						"Action":   []string{"ssm:GetCommandInvocation", "ssm:ListCommandInvocations"},
						"Resource": "*",
					},
				},
			}
			b, err := json.Marshal(doc)
			return string(b), err
		}).(pulumi.StringOutput)

		if _, err := iam.NewRolePolicy(ctx, "ci-ecr-push", &iam.RolePolicyArgs{
			Role:   ciRole.Name,
			Policy: rolePolicy,
		}, awsProviderOpt); err != nil {
			return err
		}

		// -----------------------------------------------------------------------------------------
		// Outputs — what CI and PLATFORM consume after `pulumi up`.
		// -----------------------------------------------------------------------------------------

		// The ECR repository URL (…dkr.ecr.<region>.amazonaws.com/<imageName>). CI pushes
		// `${repoUrl}:<sha>` + `${repoUrl}:latest`; PLATFORM sets `serverImage = ${repoUrl}:<gitRef>`.
		ctx.Export("repoUrl", repo.RepositoryUrl)
		// The repository ARN (for reference / cross-account grants if ever needed).
		ctx.Export("repoArn", repo.Arn)
		// The CI role ARN — CI's `role-to-assume`. Drop it into the repo's AWS_ROLE_ARN variable.
		ctx.Export("ciRoleArn", ciRole.Arn)
		// The OIDC provider ARN (informational — the trust references it).
		ctx.Export("githubOidcProviderArn", githubOidc.Arn)
		// The region the foundation lives in — handy when wiring CI / PLATFORM to match.
		ctx.Export("ecrRegion", pulumi.String(region))
		// The SSM document CI sends to the box to roll it in place (deploy-image.yml → recollect-update).
		ctx.Export("ssmUpdateDocument", updateDoc.Name)

		return nil
	})
}
