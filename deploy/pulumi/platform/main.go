// Recollect — PLATFORM: the lean launch host as code (tech-design §10.1).
//
// Run PER RELEASE (`pulumi up`). The per-deployment, frequently-changing half of the launch infra;
// its companion FOUNDATION stack (run once — deploy/pulumi/foundation/) owns the account-level
// scaffolding PLATFORM depends on: the ECR repo this box PULLS the server image from, plus the
// GitHub-OIDC CI role that built + pushed that image. Splitting the two Pulumi projects/states means
// a routine PLATFORM `pulumi up` can never replace the ECR repo or the CI trust.
//
// Provisions, in one `pulumi up`:
//
//	AWS         a single free-tier EC2 box, an EGRESS-ONLY security group (the tunnel dials out, so
//	            no inbound ports), an SSM + CloudWatch-agent + ECR-read-only instance role (keyless
//	            admin, host metrics, AND a keyless `docker pull` of the server image), a DURABLE EBS
//	            data volume (Postgres + observability state survive a box replace), cloud-init that
//	            PULLS the image and brings up the deploy compose stack, AWS Budgets free-tier
//	            guardrails, and CloudWatch out-of-band box-health alarms → an SNS email topic.
//	Cloudflare  a named Tunnel (Pulumi reads back its connector token into user-data), its ingress
//	            config (play.<domain> → the game server, grafana.<domain> → the on-box Grafana),
//	            proxied DNS CNAMEs, a Zero Trust ACCESS app + allow policy gating Grafana to the
//	            maintainer email, and the static WEBSITE on Cloudflare Pages (direct-upload project +
//	            apex/www custom-domain bindings + apex/www DNS). Apex/www is the website; play./
//	            grafana. are the box — one zone, no collision. Edge TLS is Cloudflare's.
//
// Every AWS resource is TAGGED via the provider's defaultTags + a per-resource Name. Cloudflare
// resources can't take the AWS tag set: DNS records carry a `comment` marker; the tunnel, Access
// app/policy, and Pages project/domains have no comment/tag field. See deploy/README.md "Tagging".
//
// NOTHING deployment-unique (domain, repo URL, Cloudflare account/zone ids, maintainer email, region)
// is committed here — those are the operator's required/optional config, supplied at deploy time, so
// this repo stays GENERIC. No click-ops; the box is reproducible.
package main

import (
	"bytes"
	"compress/gzip"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/budgets"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/cloudwatch"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/ebs"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/ec2"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/iam"
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/sns"
	"github.com/pulumi/pulumi-cloudflare/sdk/v6/go/cloudflare"
	"github.com/pulumi/pulumi-random/sdk/v4/go/random"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

func main() {
	pulumi.Run(func(ctx *pulumi.Context) error {
		cfg := config.New(ctx, "")

		// getInt reads an integer config, honoring an explicit value (INCLUDING 0, e.g. swapSizeGb=0 to
		// disable swap) and falling back to def only when the key is genuinely unset — matching the TS
		// `cfg.getNumber(...) ?? def` (a plain cfg.GetInt can't tell an unset key from an explicit 0).
		getInt := func(key string, def int) int {
			if s := cfg.Get(key); s != "" {
				if v, err := strconv.Atoi(s); err == nil {
					return v
				}
			}
			return def
		}

		// `region` defaults to us-east-2 (free-tier); everything BELOW that identifies a specific
		// deployment is operator-supplied, so no real value is committed.
		region := cfg.Get("region")
		if region == "" {
			region = "us-east-2"
		}
		instanceType := cfg.Get("instanceType")
		if instanceType == "" {
			instanceType = "t3.micro"
		}
		// REQUIRED — the public hostname, the git URL cloud-init clones, the pinned ref, and the ECR
		// server image ref the box pulls (the FOUNDATION/PLATFORM split; production doesn't build on-box).
		domain := cfg.Require("domain")
		repoUrl := cfg.Require("repoUrl")
		gitRef := cfg.Require("gitRef") // a pinned SHA/tag — never a moving branch
		serverImage := cfg.Require("serverImage")
		cfAccountId := cfg.Require("cloudflareAccountId")
		cfZoneId := cfg.Require("cloudflareZoneId")
		// The host part for the GAME SERVER (default play ⇒ play.<domain>); the apex + www belong to
		// the static website on Cloudflare Pages, so the two share the one zone without colliding.
		gameSubdomain := cfg.Get("gameSubdomain")
		if gameSubdomain == "" {
			gameSubdomain = "play"
		}
		gameHostname := gameSubdomain + "." + domain
		// The Cloudflare Pages PROJECT NAME (direct-upload; CI wrangler-uploads dist/). Generic default;
		// exported as `pagesProjectName` so CI's CF_PAGES_PROJECT var references the same project.
		pagesName := cfg.Get("pagesProjectName")
		if pagesName == "" {
			pagesName = "recollect-site"
		}
		// OTLP endpoint. EMPTY ⇒ the compose points the server at the ON-BOX lgtm stack; set it only to
		// ship OFF-box instead.
		otelEndpoint := cfg.Get("otelEndpoint")
		// Cloudflare Web Analytics beacon token (OPTIONAL, secret). Unset ⇒ a secret "" (no beacon).
		cfBeaconToken := cfg.GetSecret("cfBeaconToken")
		// Sizes (GiB): the durable /data volume, the instance root, and the swap file on /data. The
		// defaults keep root (10) + /data (20) = 30 GiB inside the 30 GB/12-month EBS free tier.
		dataVolumeSizeGb := getInt("dataVolumeSizeGb", 20)
		rootVolumeSizeGb := getInt("rootVolumeSizeGb", 10)
		swapSizeGb := getInt("swapSizeGb", 4)
		// Optional email for the AWS Budgets free-tier alerts; the monthly cap in USD.
		budgetEmail := cfg.Get("budgetEmail")
		monthlyBudgetUsd := cfg.Get("monthlyBudgetUsd")
		if monthlyBudgetUsd == "" {
			monthlyBudgetUsd = "5"
		}
		// REQUIRED — the maintainer email allowed through Cloudflare Access to reach grafana.<domain>.
		maintainerEmail := cfg.Require("maintainerEmail")
		grafanaSubdomain := cfg.Get("grafanaSubdomain")
		if grafanaSubdomain == "" {
			grafanaSubdomain = "grafana"
		}
		grafanaHostname := grafanaSubdomain + "." + domain
		// OPTIONAL (R2-1, defense-in-depth) — the Cloudflare Zero Trust team name. When SET, the grafana
		// tunnel ingress validates the Access JWT at the connector too; UNSET ⇒ edge-only Access.
		cfTeamName := cfg.Get("cfTeamName")
		// Email for the CloudWatch out-of-band box-health alarms (SNS); falls back to budgetEmail.
		alarmEmail := cfg.Get("alarmEmail")
		if alarmEmail == "" {
			alarmEmail = budgetEmail
		}
		cpuAlarmThresholdPct := getInt("cpuAlarmThresholdPct", 80)
		environment := cfg.Get("environment")
		if environment == "" {
			environment = "production"
		}

		// Tagging — one common set on EVERY AWS resource via the provider's defaultTags. Repository
		// REUSES the repoUrl config. The per-resource `Name` is added on each key resource below.
		commonTags := pulumi.StringMap{
			"Project":     pulumi.String("recollect"),
			"Environment": pulumi.String(environment),
			"ManagedBy":   pulumi.String("pulumi"),
			"Stack":       pulumi.String(ctx.Project()),
			"Repository":  pulumi.String(repoUrl),
		}
		// The provenance marker for the one Cloudflare resource kind that takes a free-form comment (DNS).
		const cfManagedComment = "managed by Pulumi — recollect"

		// Region-scoped AWS provider; defaultTags applies commonTags to every taggable resource.
		awsProvider, err := aws.NewProvider(ctx, "aws", &aws.ProviderArgs{
			Region:      pulumi.String(region),
			DefaultTags: &aws.ProviderDefaultTagsArgs{Tags: commonTags},
		})
		if err != nil {
			return err
		}
		awsProviderOpt := pulumi.Provider(awsProvider)

		// -----------------------------------------------------------------------------------------
		// Cloudflare — the named Tunnel, its config, and the DNS route.
		// -----------------------------------------------------------------------------------------

		// The tunnel's shared secret: 40 random bytes, base64. Pulumi-generated + stored as a secret.
		tunnelSecret, err := random.NewRandomBytes(ctx, "tunnel-secret", &random.RandomBytesArgs{
			Length: pulumi.Int(40),
		})
		if err != nil {
			return err
		}

		tunnel, err := cloudflare.NewZeroTrustTunnelCloudflared(ctx, "recollect", &cloudflare.ZeroTrustTunnelCloudflaredArgs{
			AccountId: pulumi.String(cfAccountId),
			Name:      pulumi.String("recollect"),
			// We manage the tunnel's ingress declaratively below (configSrc = cloudflare).
			ConfigSrc:    pulumi.String("cloudflare"),
			TunnelSecret: tunnelSecret.Base64,
		})
		if err != nil {
			return err
		}

		// The connector token the cloudflared container runs with. Read back from the tunnel so Pulumi
		// owns the whole lifecycle (no manual token copy/paste). Wrapped as a secret.
		tunnelToken := pulumi.ToSecret(cloudflare.GetZeroTrustTunnelCloudflaredTokenOutput(ctx, cloudflare.GetZeroTrustTunnelCloudflaredTokenOutputArgs{
			AccountId: pulumi.String(cfAccountId),
			TunnelId:  tunnel.ID(),
		}).Token()).(pulumi.StringOutput)

		// -----------------------------------------------------------------------------------------
		// Cloudflare Access (Zero Trust) — the gate in front of Grafana. The tunnel can REACH Grafana,
		// but Access authenticates the visitor as an allowed email BEFORE any request reaches the
		// origin, so grafana.<domain> resolves yet is NEVER publicly usable. Defined BEFORE the tunnel
		// ingress so the optional R2-1 origin-JWT block can bind the connector's check to this app's AUD.
		// -----------------------------------------------------------------------------------------

		// The allow policy: ALLOW exactly the maintainer email; everyone else is denied by default.
		grafanaAccessPolicy, err := cloudflare.NewZeroTrustAccessPolicy(ctx, "grafana-maintainer", &cloudflare.ZeroTrustAccessPolicyArgs{
			AccountId: pulumi.String(cfAccountId),
			Name:      pulumi.String("Recollect Grafana — maintainer only"),
			Decision:  pulumi.String("allow"),
			Includes: cloudflare.ZeroTrustAccessPolicyIncludeArray{
				&cloudflare.ZeroTrustAccessPolicyIncludeArgs{
					Email: &cloudflare.ZeroTrustAccessPolicyIncludeEmailArgs{Email: pulumi.String(maintainerEmail)},
				},
			},
		})
		if err != nil {
			return err
		}

		// The self-hosted Access application bound to grafana.<domain>, referencing the allow policy.
		// Its `.Aud` feeds the optional R2-1 origin-JWT validation on the tunnel ingress below.
		grafanaAccessApp, err := cloudflare.NewZeroTrustAccessApplication(ctx, "grafana", &cloudflare.ZeroTrustAccessApplicationArgs{
			AccountId:               pulumi.String(cfAccountId),
			Name:                    pulumi.String("Recollect Grafana"),
			Domain:                  pulumi.String(grafanaHostname),
			Type:                    pulumi.String("self_hosted"),
			SessionDuration:         pulumi.String("24h"),
			AppLauncherVisible:      pulumi.Bool(true),
			HttpOnlyCookieAttribute: pulumi.Bool(true),
			SameSiteCookieAttribute: pulumi.String("lax"),
			Policies: cloudflare.ZeroTrustAccessApplicationPolicyArray{
				&cloudflare.ZeroTrustAccessApplicationPolicyArgs{Id: grafanaAccessPolicy.ID(), Precedence: pulumi.Int(1)},
			},
		})
		if err != nil {
			return err
		}

		// Tunnel ingress: play.<domain> → the game server; grafana.<domain> → the on-box Grafana, GATED
		// by the Access app. When cfTeamName is set (R2-1), the grafana ingress adds an origin-JWT check
		// at the connector (defense-in-depth). The trailing catch-all returns 404 off-hostname.
		grafanaIngress := &cloudflare.ZeroTrustTunnelCloudflaredConfigConfigIngressArgs{
			Hostname: pulumi.String(grafanaHostname),
			Service:  pulumi.String("http://lgtm:3000"),
		}
		if cfTeamName != "" {
			grafanaIngress.OriginRequest = &cloudflare.ZeroTrustTunnelCloudflaredConfigConfigIngressOriginRequestArgs{
				Access: &cloudflare.ZeroTrustTunnelCloudflaredConfigConfigIngressOriginRequestAccessArgs{
					Required: pulumi.Bool(true),
					TeamName: pulumi.String(cfTeamName),
					AudTags:  pulumi.StringArray{grafanaAccessApp.Aud},
				},
			}
		}
		if _, err := cloudflare.NewZeroTrustTunnelCloudflaredConfig(ctx, "recollect", &cloudflare.ZeroTrustTunnelCloudflaredConfigArgs{
			AccountId: pulumi.String(cfAccountId),
			TunnelId:  tunnel.ID(),
			Config: &cloudflare.ZeroTrustTunnelCloudflaredConfigConfigArgs{
				Ingresses: cloudflare.ZeroTrustTunnelCloudflaredConfigConfigIngressArray{
					&cloudflare.ZeroTrustTunnelCloudflaredConfigConfigIngressArgs{
						Hostname: pulumi.String(gameHostname),
						Service:  pulumi.String("http://server:8080"),
					},
					grafanaIngress,
					&cloudflare.ZeroTrustTunnelCloudflaredConfigConfigIngressArgs{
						Service: pulumi.String("http_status:404"),
					},
				},
			},
		}); err != nil {
			return err
		}

		// Proxied DNS: play.<domain> + grafana.<domain> → <tunnel-id>.cfargotunnel.com (edge TLS + CDN +
		// WebSocket proxying). The apex + www records live in the Cloudflare Pages block below.
		tunnelTarget := pulumi.Sprintf("%s.cfargotunnel.com", tunnel.ID())
		if _, err := cloudflare.NewDnsRecord(ctx, "recollect-game", &cloudflare.DnsRecordArgs{
			ZoneId:  pulumi.String(cfZoneId),
			Name:    pulumi.String(gameHostname),
			Type:    pulumi.String("CNAME"),
			Content: tunnelTarget,
			Ttl:     pulumi.Float64(1), // 1 = "automatic"; required while proxied.
			Proxied: pulumi.Bool(true),
			Comment: pulumi.String(cfManagedComment),
		}); err != nil {
			return err
		}
		if _, err := cloudflare.NewDnsRecord(ctx, "recollect-grafana", &cloudflare.DnsRecordArgs{
			ZoneId:  pulumi.String(cfZoneId),
			Name:    pulumi.String(grafanaHostname),
			Type:    pulumi.String("CNAME"),
			Content: tunnelTarget,
			Ttl:     pulumi.Float64(1),
			Proxied: pulumi.Bool(true),
			Comment: pulumi.String(cfManagedComment),
		}); err != nil {
			return err
		}

		// -----------------------------------------------------------------------------------------
		// Cloudflare Pages — the static WEBSITE (apex + www), all as IaC. CI `wrangler pages deploy`s
		// the built dist/ to a DIRECT-UPLOAD project (no git connection). This block makes the project,
		// the two custom-domain bindings, and the two zone DNS CNAMEs reproducible.
		// -----------------------------------------------------------------------------------------

		// OMITTING a git source is what makes it a DIRECT-UPLOAD project. Exported below so CI's
		// CF_PAGES_PROJECT names the SAME project Pulumi created.
		pagesProject, err := cloudflare.NewPagesProject(ctx, "recollect-site", &cloudflare.PagesProjectArgs{
			AccountId:        pulumi.String(cfAccountId),
			Name:             pulumi.String(pagesName),
			ProductionBranch: pulumi.String("main"),
		})
		if err != nil {
			return err
		}

		// The custom-domain bindings (apex + www) tell Pages to SERVE those hostnames + mint their edge
		// certs — SEPARATE from the zone DNS records (next); Cloudflare requires both.
		wwwHostname := "www." + domain
		if _, err := cloudflare.NewPagesDomain(ctx, "recollect-site-apex", &cloudflare.PagesDomainArgs{
			AccountId:   pulumi.String(cfAccountId),
			ProjectName: pagesProject.Name,
			Name:        pulumi.String(domain),
		}); err != nil {
			return err
		}
		if _, err := cloudflare.NewPagesDomain(ctx, "recollect-site-www", &cloudflare.PagesDomainArgs{
			AccountId:   pulumi.String(cfAccountId),
			ProjectName: pagesProject.Name,
			Name:        pulumi.String(wwwHostname),
		}); err != nil {
			return err
		}

		// The zone DNS: apex + www → the project's <name>.pages.dev origin, proxied (edge TLS + CDN). At
		// the apex this is CNAME flattening, which Cloudflare handles transparently. subdomain is an
		// output of the project resource, so the records always track the real target with no hardcoding.
		if _, err := cloudflare.NewDnsRecord(ctx, "recollect-site-apex", &cloudflare.DnsRecordArgs{
			ZoneId:  pulumi.String(cfZoneId),
			Name:    pulumi.String(domain),
			Type:    pulumi.String("CNAME"),
			Content: pagesProject.Subdomain,
			Ttl:     pulumi.Float64(1),
			Proxied: pulumi.Bool(true),
			Comment: pulumi.String(cfManagedComment),
		}); err != nil {
			return err
		}
		if _, err := cloudflare.NewDnsRecord(ctx, "recollect-site-www", &cloudflare.DnsRecordArgs{
			ZoneId:  pulumi.String(cfZoneId),
			Name:    pulumi.String(wwwHostname),
			Type:    pulumi.String("CNAME"),
			Content: pagesProject.Subdomain,
			Ttl:     pulumi.Float64(1),
			Proxied: pulumi.Bool(true),
			Comment: pulumi.String(cfManagedComment),
		}); err != nil {
			return err
		}

		// -----------------------------------------------------------------------------------------
		// AWS — the EC2 box, its keyless-admin role, the egress-only SG, and cloud-init.
		// -----------------------------------------------------------------------------------------

		// Latest Amazon Linux 2023 AMI for x86_64 (t3), looked up so the box tracks security patches.
		ami := ec2.LookupAmiOutput(ctx, ec2.LookupAmiOutputArgs{
			Owners:     pulumi.StringArray{pulumi.String("amazon")},
			MostRecent: pulumi.Bool(true),
			Filters: ec2.GetAmiFilterArray{
				&ec2.GetAmiFilterArgs{Name: pulumi.String("name"), Values: pulumi.StringArray{pulumi.String("al2023-ami-2023.*-x86_64")}},
				&ec2.GetAmiFilterArgs{Name: pulumi.String("virtualization-type"), Values: pulumi.StringArray{pulumi.String("hvm")}},
				&ec2.GetAmiFilterArgs{Name: pulumi.String("state"), Values: pulumi.StringArray{pulumi.String("available")}},
			},
		}, awsProviderOpt)

		// Keyless admin via SSM Session Manager: an instance role with the managed SSM policy — no SSH
		// key, no inbound 22.
		assumeRoleDoc, err := json.Marshal(map[string]any{
			"Version": "2012-10-17",
			"Statement": []any{
				map[string]any{
					"Action":    "sts:AssumeRole",
					"Effect":    "Allow",
					"Principal": map[string]any{"Service": "ec2.amazonaws.com"},
				},
			},
		})
		if err != nil {
			return err
		}
		role, err := iam.NewRole(ctx, "recollect-ec2", &iam.RoleArgs{
			AssumeRolePolicy: pulumi.String(string(assumeRoleDoc)),
			Tags:             pulumi.StringMap{"Name": pulumi.String("recollect-instance-role")},
		}, awsProviderOpt)
		if err != nil {
			return err
		}
		// SSM (keyless admin), the CloudWatch agent (the §11 custom host metrics), and ECR read-only
		// (the box pulls the server image FOUNDATION's CI pushed — no stored creds, and it can never
		// push or mutate a repo).
		for name, arn := range map[string]string{
			"recollect-ssm":      "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore",
			"recollect-cwagent":  "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy",
			"recollect-ecr-pull": "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly",
		} {
			if _, err := iam.NewRolePolicyAttachment(ctx, name, &iam.RolePolicyAttachmentArgs{
				Role:      role.Name,
				PolicyArn: pulumi.String(arn),
			}, awsProviderOpt); err != nil {
				return err
			}
		}
		instanceProfile, err := iam.NewInstanceProfile(ctx, "recollect-ec2", &iam.InstanceProfileArgs{
			Role: role.Name,
			Tags: pulumi.StringMap{"Name": pulumi.String("recollect-instance-profile")},
		}, awsProviderOpt)
		if err != nil {
			return err
		}

		// Default VPC to place the box (the free-tier story doesn't need a bespoke VPC).
		defaultVpc := ec2.LookupVpcOutput(ctx, ec2.LookupVpcOutputArgs{Default: pulumi.Bool(true)}, awsProviderOpt)

		// EGRESS-ONLY security group: the Cloudflare Tunnel dials OUT, so no inbound rule at all. Egress
		// is left wide so apt/docker/cloudflared/Postgres reach what they need.
		sg, err := ec2.NewSecurityGroup(ctx, "recollect", &ec2.SecurityGroupArgs{
			Description: pulumi.String("Recollect launch host - egress only; ingress via Cloudflare Tunnel (no inbound)."),
			VpcId:       defaultVpc.Id(),
			Egress: ec2.SecurityGroupEgressArray{
				&ec2.SecurityGroupEgressArgs{
					Protocol:       pulumi.String("-1"),
					FromPort:       pulumi.Int(0),
					ToPort:         pulumi.Int(0),
					CidrBlocks:     pulumi.StringArray{pulumi.String("0.0.0.0/0")},
					Ipv6CidrBlocks: pulumi.StringArray{pulumi.String("::/0")},
					Description:    pulumi.String("all egress (tunnel dials out; package + image pulls)"),
				},
			},
			Tags: pulumi.StringMap{"Name": pulumi.String("recollect-sg")},
		}, awsProviderOpt)
		if err != nil {
			return err
		}

		// On-box Postgres password: GENERATED, never an operator input (Postgres has no published port).
		// 40 URL-safe alphanumerics (no specials) so it drops into the DSN with no percent-encoding
		// hazard. `.Result` is a secret output — encrypted in state + the rendered user-data.
		pgResource, err := random.NewRandomPassword(ctx, "postgres-password", &random.RandomPasswordArgs{
			Length:  pulumi.Int(40),
			Special: pulumi.Bool(false),
		})
		if err != nil {
			return err
		}
		pgPassword := pgResource.Result

		// Render the cloud-init script: read the committed template and substitute the @@PLACEHOLDERS@@,
		// then gzip + base64 it. EC2 caps user_data at 16 KiB (raw); the ~17.4 KiB script ships gzip-
		// COMPRESSED (cloud-init on AL2023 detects the magic bytes and decompresses). Go's gzip writer
		// already emits a deterministic header (mtime 0, OS 255), so the bytes are identical across
		// whatever machine runs `pulumi up` — no spurious userDataReplaceOnChange. Deriving via Apply on
		// the secret token/password keeps the whole payload a tracked SECRET.
		userDataTemplate, err := os.ReadFile("user-data.sh")
		if err != nil {
			return err
		}
		userDataBase64 := pulumi.All(tunnelToken, pgPassword, cfBeaconToken).ApplyT(func(vals []interface{}) (string, error) {
			token, _ := vals[0].(string)
			pgPass, _ := vals[1].(string)
			beacon, _ := vals[2].(string)
			script := strings.NewReplacer(
				"@@REPO_URL@@", repoUrl,
				"@@GIT_REF@@", gitRef,
				// The ECR server image the box PULLS: cloud-init writes it as IMAGE_REF in .env, logs in
				// to ECR with the instance role, and `compose pull`s it — no on-box Rust build.
				"@@IMAGE_REF@@", serverImage,
				// The play client the BOX serves dials the game origin (play.<domain>) same-origin for wss.
				"@@SITE_ORIGIN@@", "https://"+gameHostname,
				"@@TUNNEL_TOKEN@@", token,
				"@@POSTGRES_PASSWORD@@", pgPass,
				"@@OTEL_ENDPOINT@@", otelEndpoint,
				"@@CF_BEACON_TOKEN@@", beacon,
				"@@SWAP_SIZE_GB@@", strconv.Itoa(swapSizeGb),
				// The bare domain the compose builds the Grafana root URL from (https://grafana.<domain>).
				"@@OBS_GRAFANA_DOMAIN@@", domain,
			).Replace(string(userDataTemplate))

			var buf bytes.Buffer
			gz, err := gzip.NewWriterLevel(&buf, gzip.BestCompression)
			if err != nil {
				return "", err
			}
			gz.OS = 0xff // "unknown" — keep the header platform-independent (Go's default, made explicit)
			if _, err := gz.Write([]byte(script)); err != nil {
				return "", err
			}
			if err := gz.Close(); err != nil {
				return "", err
			}
			return base64.StdEncoding.EncodeToString(buf.Bytes()), nil
		}).(pulumi.StringOutput)

		instance, err := ec2.NewInstance(ctx, "recollect", &ec2.InstanceArgs{
			Ami:                 ami.Id(),
			InstanceType:        ec2.InstanceType(instanceType),
			IamInstanceProfile:  instanceProfile.Name,
			VpcSecurityGroupIds: pulumi.StringArray{sg.ID()},
			// No subnetId ⇒ the default subnet. It gets a public IP (free egress for pulls), but the SG
			// has ZERO inbound rules, so that IP is unreachable — the only way in is the outbound tunnel.
			RootBlockDevice: &ec2.InstanceRootBlockDeviceArgs{
				VolumeSize: pulumi.Int(rootVolumeSizeGb),
				VolumeType: pulumi.String("gp3"),
				Encrypted:  pulumi.Bool(true),
			},
			UserDataBase64: userDataBase64,
			// Re-render + replace the box when the bootstrap inputs change (e.g. a new pinned gitRef).
			UserDataReplaceOnChange: pulumi.Bool(true),
			// IMDSv2 required (tokens), hop limit 1 — block SSRF-to-metadata.
			MetadataOptions: &ec2.InstanceMetadataOptionsArgs{
				HttpTokens:              pulumi.String("required"),
				HttpPutResponseHopLimit: pulumi.Int(1),
			},
			Tags: pulumi.StringMap{"Name": pulumi.String("recollect-server")},
		}, awsProviderOpt)
		if err != nil {
			return err
		}

		// -----------------------------------------------------------------------------------------
		// AWS — the DURABLE data volume (state that must survive the box: Postgres + observability).
		// A standalone ebs.Volume (not an instance ebsBlockDevice) has its own lifecycle and is NEVER
		// deleted on instance termination: a box REPLACE only replaces the VolumeAttachment below, so
		// the Volume stays put and the new box re-binds it. It MUST sit in the instance's AZ (EBS is
		// AZ-local). No `protect`/`retainOnDelete` — a real destroy should still delete it cleanly.
		// -----------------------------------------------------------------------------------------
		dataVolume, err := ebs.NewVolume(ctx, "recollect-data", &ebs.VolumeArgs{
			AvailabilityZone: instance.AvailabilityZone,
			Size:             pulumi.Int(dataVolumeSizeGb),
			Type:             pulumi.String("gp3"),
			Encrypted:        pulumi.Bool(true),
			Tags: pulumi.StringMap{
				"Name":           pulumi.String("recollect-data-volume"),
				"recollect:data": pulumi.String("true"),
			},
		}, awsProviderOpt)
		if err != nil {
			return err
		}
		// Attach at /dev/sdf (Nitro exposes it as an NVMe device user-data resolves from /dev/sdf).
		// deleteOnTermination defaults FALSE — a separate volume's attachment never tears it down.
		// DeleteBeforeReplace: on a box REPLACE the volume must move to the new instance, but an EBS
		// volume can only be attached to ONE instance at a time. Pulumi's default create-before-delete
		// would try to attach the (still-attached) volume to the new box first → `VolumeInUse`. Deleting
		// the OLD attachment first detaches the volume from the outgoing box, then it attaches to the new
		// one — the durable-volume-across-replace flow works cleanly.
		if _, err := ec2.NewVolumeAttachment(ctx, "recollect-data", &ec2.VolumeAttachmentArgs{
			DeviceName: pulumi.String("/dev/sdf"),
			VolumeId:   dataVolume.ID(),
			InstanceId: instance.ID(),
		}, awsProviderOpt, pulumi.DeleteBeforeReplace(true)); err != nil {
			return err
		}

		// -----------------------------------------------------------------------------------------
		// AWS Budgets — free-tier guardrails with email alerts (so a mistake can't run up a bill).
		// Notify at 80% actual and 100% forecast of the monthly cap (both email when budgetEmail is set).
		// -----------------------------------------------------------------------------------------
		var monthlyNotifications budgets.BudgetNotificationArray
		if budgetEmail != "" {
			monthlyNotifications = budgets.BudgetNotificationArray{
				&budgets.BudgetNotificationArgs{
					ComparisonOperator:       pulumi.String("GREATER_THAN"),
					Threshold:                pulumi.Float64(80),
					ThresholdType:            pulumi.String("PERCENTAGE"),
					NotificationType:         pulumi.String("ACTUAL"),
					SubscriberEmailAddresses: pulumi.StringArray{pulumi.String(budgetEmail)},
				},
				&budgets.BudgetNotificationArgs{
					ComparisonOperator:       pulumi.String("GREATER_THAN"),
					Threshold:                pulumi.Float64(100),
					ThresholdType:            pulumi.String("PERCENTAGE"),
					NotificationType:         pulumi.String("FORECASTED"),
					SubscriberEmailAddresses: pulumi.StringArray{pulumi.String(budgetEmail)},
				},
			}
		}
		if _, err := budgets.NewBudget(ctx, "recollect-monthly", &budgets.BudgetArgs{
			BudgetType:    pulumi.String("COST"),
			TimeUnit:      pulumi.String("MONTHLY"),
			LimitAmount:   pulumi.String(monthlyBudgetUsd),
			LimitUnit:     pulumi.String("USD"),
			Notifications: monthlyNotifications,
			Tags:          pulumi.StringMap{"Name": pulumi.String("recollect-monthly-budget")},
		}, awsProviderOpt); err != nil {
			return err
		}

		// A dedicated low-dollar guardrail: the moment ACTUAL monthly spend crosses $1 (you've left $0
		// free-tier territory), this fires — a tighter, actual-cost tripwire than the forecasted budget.
		var guardNotifications budgets.BudgetNotificationArray
		if budgetEmail != "" {
			guardNotifications = budgets.BudgetNotificationArray{
				&budgets.BudgetNotificationArgs{
					ComparisonOperator:       pulumi.String("GREATER_THAN"),
					Threshold:                pulumi.Float64(1),
					ThresholdType:            pulumi.String("ABSOLUTE_VALUE"),
					NotificationType:         pulumi.String("ACTUAL"),
					SubscriberEmailAddresses: pulumi.StringArray{pulumi.String(budgetEmail)},
				},
			}
		}
		if _, err := budgets.NewBudget(ctx, "recollect-free-tier-guard", &budgets.BudgetArgs{
			BudgetType:    pulumi.String("COST"),
			TimeUnit:      pulumi.String("MONTHLY"),
			LimitAmount:   pulumi.String("1"),
			LimitUnit:     pulumi.String("USD"),
			Notifications: guardNotifications,
			Tags:          pulumi.StringMap{"Name": pulumi.String("recollect-free-tier-guard-budget")},
		}, awsProviderOpt); err != nil {
			return err
		}

		// -----------------------------------------------------------------------------------------
		// CloudWatch — the OUT-OF-BAND box-health net (tech-design §11): the second, independent eye
		// (the in-box Grafana can't alarm on its own outage). 7 alarms (≤10 free) on AWS-native EC2
		// metrics + the custom host metrics the on-box CloudWatch agent publishes, → an SNS email topic.
		// -----------------------------------------------------------------------------------------

		alarmTopic, err := sns.NewTopic(ctx, "recollect-alarms", &sns.TopicArgs{
			Name: pulumi.String("recollect-alarms"),
			Tags: pulumi.StringMap{"Name": pulumi.String("recollect-alarms")},
		}, awsProviderOpt)
		if err != nil {
			return err
		}
		if alarmEmail != "" {
			if _, err := sns.NewTopicSubscription(ctx, "recollect-alarms-email", &sns.TopicSubscriptionArgs{
				Topic:    alarmTopic.Arn,
				Protocol: pulumi.String("email"),
				Endpoint: pulumi.String(alarmEmail),
			}, awsProviderOpt); err != nil {
				return err
			}
		}

		dims := pulumi.StringMap{"InstanceId": instance.ID()}
		alarmActions := pulumi.Array{alarmTopic.Arn}

		// 1. EC2 instance status check failed — the guest/OS is unhealthy.
		if _, err := cloudwatch.NewMetricAlarm(ctx, "recollect-status-instance", &cloudwatch.MetricAlarmArgs{
			AlarmDescription:   pulumi.String("EC2 instance status check failed (OS/guest unhealthy)."),
			Namespace:          pulumi.String("AWS/EC2"),
			MetricName:         pulumi.String("StatusCheckFailed_Instance"),
			Dimensions:         dims,
			Statistic:          pulumi.String("Maximum"),
			Period:             pulumi.Int(300),
			EvaluationPeriods:  pulumi.Int(2),
			Threshold:          pulumi.Float64(1),
			ComparisonOperator: pulumi.String("GreaterThanOrEqualToThreshold"),
			TreatMissingData:   pulumi.String("missing"),
			AlarmActions:       alarmActions,
			OkActions:          alarmActions,
			Tags:               pulumi.StringMap{"Name": pulumi.String("recollect-status-instance")},
		}, awsProviderOpt); err != nil {
			return err
		}

		// 2. EC2 SYSTEM status check failed — the underlying host is unhealthy; add the free built-in
		// EC2 `recover` action so AWS auto-recovers the instance onto healthy hardware.
		if _, err := cloudwatch.NewMetricAlarm(ctx, "recollect-status-system", &cloudwatch.MetricAlarmArgs{
			AlarmDescription:   pulumi.String("EC2 system status check failed (underlying host unhealthy) - auto-recovers."),
			Namespace:          pulumi.String("AWS/EC2"),
			MetricName:         pulumi.String("StatusCheckFailed_System"),
			Dimensions:         dims,
			Statistic:          pulumi.String("Maximum"),
			Period:             pulumi.Int(300),
			EvaluationPeriods:  pulumi.Int(2),
			Threshold:          pulumi.Float64(1),
			ComparisonOperator: pulumi.String("GreaterThanOrEqualToThreshold"),
			TreatMissingData:   pulumi.String("missing"),
			AlarmActions:       pulumi.Array{alarmTopic.Arn, pulumi.String(fmt.Sprintf("arn:aws:automate:%s:ec2:recover", region))},
			OkActions:          alarmActions,
			Tags:               pulumi.StringMap{"Name": pulumi.String("recollect-status-system")},
		}, awsProviderOpt); err != nil {
			return err
		}

		// 3. CPUUtilization sustained high — on a t3.micro this also burns CPU credits; the box idles
		// low, so a sustained pin is a real signal.
		if _, err := cloudwatch.NewMetricAlarm(ctx, "recollect-cpu-high", &cloudwatch.MetricAlarmArgs{
			AlarmDescription:   pulumi.String(fmt.Sprintf("EC2 CPUUtilization >= %d%% sustained.", cpuAlarmThresholdPct)),
			Namespace:          pulumi.String("AWS/EC2"),
			MetricName:         pulumi.String("CPUUtilization"),
			Dimensions:         dims,
			Statistic:          pulumi.String("Average"),
			Period:             pulumi.Int(300),
			EvaluationPeriods:  pulumi.Int(3),
			Threshold:          pulumi.Float64(float64(cpuAlarmThresholdPct)),
			ComparisonOperator: pulumi.String("GreaterThanOrEqualToThreshold"),
			TreatMissingData:   pulumi.String("missing"),
			AlarmActions:       alarmActions,
			OkActions:          alarmActions,
			Tags:               pulumi.StringMap{"Name": pulumi.String("recollect-cpu-high")},
		}, awsProviderOpt); err != nil {
			return err
		}

		// 4. Memory used high — the custom mem_used_percent from the CloudWatch agent. The 1 GB box is
		// the real risk; this is the canary for "swap is about to thrash / OOM territory".
		if _, err := cloudwatch.NewMetricAlarm(ctx, "recollect-mem-high", &cloudwatch.MetricAlarmArgs{
			AlarmDescription:   pulumi.String("Host memory used >= 90% (custom CloudWatch-agent metric)."),
			Namespace:          pulumi.String("Recollect/Host"),
			MetricName:         pulumi.String("mem_used_percent"),
			Dimensions:         dims,
			Statistic:          pulumi.String("Average"),
			Period:             pulumi.Int(300),
			EvaluationPeriods:  pulumi.Int(3),
			Threshold:          pulumi.Float64(90),
			ComparisonOperator: pulumi.String("GreaterThanOrEqualToThreshold"),
			TreatMissingData:   pulumi.String("notBreaching"),
			AlarmActions:       alarmActions,
			OkActions:          alarmActions,
			Tags:               pulumi.StringMap{"Name": pulumi.String("recollect-mem-high")},
		}, awsProviderOpt); err != nil {
			return err
		}

		// 5. Swap used high — the box leans on the swap file; sustained high swap means real memory
		// pressure (consider t3.small). The companion to the mem alarm.
		if _, err := cloudwatch.NewMetricAlarm(ctx, "recollect-swap-high", &cloudwatch.MetricAlarmArgs{
			AlarmDescription:   pulumi.String("Host swap used >= 70% (custom CloudWatch-agent metric)."),
			Namespace:          pulumi.String("Recollect/Host"),
			MetricName:         pulumi.String("swap_used_percent"),
			Dimensions:         dims,
			Statistic:          pulumi.String("Average"),
			Period:             pulumi.Int(300),
			EvaluationPeriods:  pulumi.Int(3),
			Threshold:          pulumi.Float64(70),
			ComparisonOperator: pulumi.String("GreaterThanOrEqualToThreshold"),
			TreatMissingData:   pulumi.String("notBreaching"),
			AlarmActions:       alarmActions,
			OkActions:          alarmActions,
			Tags:               pulumi.StringMap{"Name": pulumi.String("recollect-swap-high")},
		}, awsProviderOpt); err != nil {
			return err
		}

		// 6 + 7. Disk space high on the two real mounts (`/` = Docker images/logs; `/data` = Postgres +
		// observability). The agent tags disk_used_percent with `path`, so each mount is its own alarm.
		for _, d := range []struct{ name, path string }{
			{"recollect-disk-root", "/"},
			{"recollect-disk-data", "/data"},
		} {
			if _, err := cloudwatch.NewMetricAlarm(ctx, d.name, &cloudwatch.MetricAlarmArgs{
				AlarmDescription:   pulumi.String(fmt.Sprintf("Host disk used >= 85%% on %s (custom CloudWatch-agent metric).", d.path)),
				Namespace:          pulumi.String("Recollect/Host"),
				MetricName:         pulumi.String("disk_used_percent"),
				Dimensions:         pulumi.StringMap{"InstanceId": instance.ID(), "path": pulumi.String(d.path)},
				Statistic:          pulumi.String("Average"),
				Period:             pulumi.Int(300),
				EvaluationPeriods:  pulumi.Int(2),
				Threshold:          pulumi.Float64(85),
				ComparisonOperator: pulumi.String("GreaterThanOrEqualToThreshold"),
				TreatMissingData:   pulumi.String("notBreaching"),
				AlarmActions:       alarmActions,
				OkActions:          alarmActions,
				Tags:               pulumi.StringMap{"Name": pulumi.String(d.name)},
			}, awsProviderOpt); err != nil {
				return err
			}
		}

		// -----------------------------------------------------------------------------------------
		// Outputs — what the operator needs after `pulumi up`.
		// -----------------------------------------------------------------------------------------
		ctx.Export("instanceId", instance.ID())
		// The durable data volume's id — it persists across box recreation; confirm the SAME volume
		// re-attached after a `pulumi up` that replaced the instance.
		ctx.Export("dataVolumeId", dataVolume.ID())
		ctx.Export("tunnelId", tunnel.ID())
		// The WEBSITE (apex, served by Cloudflare Pages) and the GAME SERVER origin (play.<domain>).
		ctx.Export("site", pulumi.Sprintf("https://%s", domain))
		ctx.Export("gameUrl", pulumi.Sprintf("https://%s", gameHostname))
		// The Cloudflare Pages PROJECT NAME + its *.pages.dev origin (wire CI's CF_PAGES_PROJECT to this).
		ctx.Export("pagesProjectName", pagesProject.Name)
		ctx.Export("pagesSubdomain", pagesProject.Subdomain)
		// The Access-gated Grafana URL (the maintainer authenticates via Cloudflare Access to reach it).
		ctx.Export("grafanaUrl", pulumi.Sprintf("https://%s", grafanaHostname))
		// The SNS topic the out-of-band CloudWatch box-health alarms publish to.
		ctx.Export("alarmTopicArn", alarmTopic.Arn)
		ctx.Export("ssmSession", pulumi.Sprintf("aws ssm start-session --region %s --target %s", region, instance.ID()))
		// The connector token + the generated Postgres password (both secrets) — surfaced encrypted for
		// debugging (`pulumi stack output … --show-secrets`).
		ctx.Export("cloudflaredToken", tunnelToken)
		ctx.Export("postgresPassword", pgPassword)

		return nil
	})
}
