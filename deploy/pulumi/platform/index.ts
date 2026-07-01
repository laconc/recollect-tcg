/**
 * Recollect — PLATFORM: the lean launch host as code (tech-design §10.1).
 *
 * Run PER RELEASE (`pulumi up`). The per-deployment, frequently-changing half of the launch infra;
 * its companion FOUNDATION stack (run once — deploy/pulumi/foundation/) owns the account-level
 * scaffolding PLATFORM depends on: the ECR repo this box PULLS the server image from, plus the
 * GitHub-OIDC CI role that built + pushed that image. Splitting the two Pulumi projects/states means
 * a routine PLATFORM `pulumi up` can never replace the ECR repo or the CI trust.
 *
 * Provisions, in one `pulumi up`:
 *   AWS         a single free-tier EC2 box in the configured region, an EGRESS-ONLY security group
 *               (the Cloudflare Tunnel dials out, so no inbound ports), an SSM + CloudWatch-agent +
 *               ECR-read-only instance role (keyless admin, host metrics, AND a keyless `docker pull`
 *               of the server image from FOUNDATION's ECR), and cloud-init user-data that PULLS the
 *               server image and brings up the deploy compose stack (recollect-server + on-box
 *               Postgres + cloudflared + the self-hosted LGTM observability stack). The on-box Rust
 *               BUILD is retired for production — the box pulls `serverImage` (the ECR ref CI pushed),
 *               never compiles on the 1 GB micro. Plus AWS Budgets guardrails so the box never
 *               silently leaves the free tier, and CloudWatch out-of-band box-health alarms → an SNS
 *               email topic (status checks, CPU, and the agent's custom memory/swap/disk metrics).
 *   Cloudflare  a named Tunnel (Pulumi creates it and reads back its connector token, injected
 *               into user-data), the Tunnel's ingress config (play.your-domain.com → the game server,
 *               and grafana.your-domain.com → the on-box Grafana), proxied DNS CNAMEs for both → the
 *               tunnel, and a Zero Trust ACCESS application + allow policy gating Grafana to the
 *               maintainer email (reachable at the subdomain, never publicly usable). PLUS the static
 *               WEBSITE on Cloudflare Pages — the direct-upload Pages PROJECT (no git connection; CI
 *               `wrangler`-uploads the built dist/), its apex + www custom-domain BINDINGS, and the
 *               apex + www DNS CNAMEs → the project's *.pages.dev. So the apex/www is the website and
 *               play./grafana. are the box — one zone, no collision. Edge TLS is Cloudflare's.
 *
 * Every AWS resource is TAGGED via the provider's defaultTags (Project/Environment/ManagedBy/Stack/
 * Repository) + a per-resource Name (see the `commonTags` block). Cloudflare resources can't take the
 * AWS tag set: the DNS records carry a `comment` marker; the tunnel, Access app/policy, and Pages
 * project/domains have no comment/tag field. See deploy/README.md "Tagging".
 *
 * Secrets/config come from typed stack config — see deploy/README.md for the exact
 * `pulumi config set [--secret] …` for every input. NOTHING deployment-unique (your domain, repo
 * URL, Cloudflare account/zone ids, maintainer email, region) is committed here: those are the
 * operator's required/optional config, supplied at deploy time, so this repo stays GENERIC —
 * anyone can deploy their own instance. No click-ops; the box is reproducible.
 */
import * as aws from "@pulumi/aws";
import * as cloudflare from "@pulumi/cloudflare";
import * as pulumi from "@pulumi/pulumi";
import * as random from "@pulumi/random";
import * as fs from "fs";
import * as path from "path";
import * as zlib from "zlib";

const cfg = new pulumi.Config();
// `region` defaults to us-east-2 (the maintainer's SSO-profile region; still free-tier) — override it
// freely; it's not deployment-unique. Everything BELOW that identifies a specific deployment (the
// domain, the repo URL, the account/zone ids, the maintainer email) is operator-supplied: no real
// value is committed, so the repo stays generic and anyone can deploy their own instance.
const region = (cfg.get("region") ?? "us-east-2") as aws.Region;
const instanceType = cfg.get("instanceType") ?? "t3.micro";
// REQUIRED — the public hostname for THIS deployment (e.g. your-domain.com). Deployment-unique, so
// there is deliberately NO committed default; set it with `pulumi config set domain your-domain.com`.
const domain = cfg.require("domain");
// REQUIRED — the git URL cloud-init clones on the box (your fork/clone of this repo). Deployment-
// unique, so no committed default; `pulumi config set repoUrl https://github.com/yourorg/recollect.git`.
const repoUrl = cfg.require("repoUrl");
const gitRef = cfg.require("gitRef"); // a pinned SHA/tag — never a moving branch
// REQUIRED — the server IMAGE the box pulls (the FOUNDATION/PLATFORM split: production no longer
// builds the server on the 1 GB box; CI builds + pushes it to ECR, the box pulls). Set it to the
// FOUNDATION stack's ECR repo URL at the pinned tag CI pushed, e.g.
//   <acct>.dkr.ecr.<region>.amazonaws.com/recollect-server:<gitRef>
// (the FOUNDATION output `repoUrl` + the `sha-<commit>` / git-SHA tag CI tagged). The box's instance
// role grants ECR read-only so `docker compose pull` works keylessly. Deployment-unique → no committed
// default. The repo stays generic; you supply your account's ECR ref. See deploy/README.md "PLATFORM".
const serverImage = cfg.require("serverImage");
const cfAccountId = cfg.require("cloudflareAccountId");
const cfZoneId = cfg.require("cloudflareZoneId");
// The host part for the GAME SERVER, prepended to <domain>. Default `play` ⇒ play.<domain>. The
// apex + `www` belong to the static WEBSITE (Cloudflare Pages — see the Pages block below); the game
// server (the wss socket + REST, served by the box behind the tunnel) lives on this sub-route, so the
// two share the one zone without colliding. The wasm play client the box serves connects same-origin,
// so it dials THIS hostname's wss. (Both deploy READMEs already declare this apex/www-vs-play split.)
const gameSubdomain = cfg.get("gameSubdomain") ?? "play";
const gameHostname = `${gameSubdomain}.${domain}`;
// The Cloudflare Pages PROJECT NAME for the static website (direct-upload; CI `wrangler pages deploy`s
// the built dist/ to it). Generic default `recollect-site` — NOT deployment-unique, so it may carry a
// default; override if you want a different project name. Exported as the `pagesProjectName` stack
// output so CI's `CF_PAGES_PROJECT` GitHub var can reference the SAME name Pulumi created. See the
// Cloudflare Pages block below + deploy/site/README.md.
const pagesName = cfg.get("pagesProjectName") ?? "recollect-site";
// OTLP endpoint for the server. EMPTY (the default) ⇒ the deploy compose points the server at the
// ON-BOX self-hosted lgtm stack (http://lgtm:4317) — the §11 launch posture. Set it only to ship
// OFF-box instead (e.g. a Grafana Cloud OTLP URL); the compose treats a set value as an override.
const otelEndpoint = cfg.get("otelEndpoint") ?? "";
const cfBeaconToken = cfg.getSecret("cfBeaconToken") ?? pulumi.output("");
// Size (GiB) of the DEDICATED, DURABLE data volume mounted at /data — the box's stateful data:
// Postgres' data dir (the match journal + accounts) AND the self-hosted observability stack's
// stores (Grafana state + the metrics/logs/traces TSDBs under /data/observability, ~1–2 GB at the
// short retention configured in the compose). It is a SEPARATE EBS volume from the instance root,
// so it survives instance termination/recreation (the whole point: a `pulumi up` that replaces the
// box re-attaches THIS volume; user-data MOUNTS, never reformats it). Default 20 GiB (ample for
// both + the swap file). See the README Cost section for the free-tier math.
const dataVolumeSizeGb = cfg.getNumber("dataVolumeSizeGb") ?? 20;
// Size (GiB) of the instance ROOT volume. Shrunk to 10 (from 30) so root + the 20 GiB /data volume
// (= 30 GiB) fit ENTIRELY within the 30 GB/12-month EBS free tier ($0) — the #31 cost fix. 10 GiB
// holds AL2023 + Docker + the built server/site images comfortably; the durable data is on /data.
const rootVolumeSizeGb = cfg.getNumber("rootVolumeSizeGb") ?? 10;
// Size (GiB) of the swap file cloud-init creates ON the durable /data volume (/data/swapfile).
// Default 4 — the 1 GB box gets RAM headroom for the self-hosted LGTM observability stack + the
// first docker build (swap is a safety net, not a default: vm.swappiness is set low). It lives on
// the data volume so it doesn't eat the root, and is counted in dataVolumeSizeGb's 20 GiB headroom.
const swapSizeGb = cfg.getNumber("swapSizeGb") ?? 4;
// Optional: an email to receive the AWS Budgets free-tier alerts. Omit to skip notifications
// (the budget + its actual/forecast thresholds still exist; they just don't email).
const budgetEmail = cfg.get("budgetEmail");
const monthlyBudgetUsd = cfg.get("monthlyBudgetUsd") ?? "5";

// --- Self-hosted observability access + alarms (tech-design §11) ------------------------------
// The maintainer email allowed through Cloudflare Access to reach grafana.<domain>. REQUIRED to
// stand up the Access-gated Grafana (the whole point is a named allow-list, not a public page).
// Set with `pulumi config set maintainerEmail you@example.com`.
const maintainerEmail = cfg.require("maintainerEmail");
// The Grafana subdomain (host part prepended to <domain>). Default `grafana` ⇒ grafana.<domain>.
const grafanaSubdomain = cfg.get("grafanaSubdomain") ?? "grafana";
const grafanaHostname = `${grafanaSubdomain}.${domain}`;
// OPTIONAL (R2-1, defense-in-depth) — your Cloudflare Zero Trust ORG/team name: the
// `<team>` in `<team>.cloudflareaccess.com`, found in the Zero Trust dashboard under
// Settings → Custom Pages (or the team-domain banner). When SET, the grafana tunnel ingress gets an
// `originRequest.access` block so the connector itself validates the `Cf-Access-Jwt-Assertion` JWT
// for the Grafana Access app's AUD — so even a request that somehow bypasses the EDGE Access check
// is rejected at the origin (anonymous-Admin Grafana is never exposed). When UNSET (the default),
// the posture is the current edge-only Access (still gated at Cloudflare; just no second origin
// check). Deployment-unique, so no committed default. See deploy/README.md "R2-1".
const cfTeamName = cfg.get("cfTeamName");
// Email for the CloudWatch out-of-band box-health alarms (SNS). Falls back to budgetEmail so one
// address covers both; omit both to create the alarms without an email subscription.
const alarmEmail = cfg.get("alarmEmail") ?? budgetEmail;
// CPUUtilization alarm threshold (%). 80% sustained on a t3.micro is the "something's wrong" line
// (the box idles low; the CPU-credit model means sustained high CPU also burns credits).
const cpuAlarmThresholdPct = cfg.getNumber("cpuAlarmThresholdPct") ?? 80;
// The deployment ENVIRONMENT this stack stands up (e.g. production, staging). Tags EVERY AWS resource
// via the provider's defaultTags below, so the console / Cost Explorer can slice by environment.
// Defaults to "production" — this launch host IS the production box; override with
// `pulumi config set environment <name>` for a non-prod copy.
const environment = cfg.get("environment") ?? "production";

// ---------------------------------------------------------------------------------------------
// Tagging — one common set on EVERY AWS resource via the provider's defaultTags (no per-resource
// tagging needed; AWS applies these to every taggable resource the provider creates). Provenance +
// an IaC-ownership signal so a human in the console knows what owns a resource and never hand-edits it:
//   Project     constant — the product these belong to.
//   Environment the `environment` config (production by default).
//   ManagedBy   constant `pulumi` — this is IaC; don't mutate it in the console.
//   Stack       this Pulumi project's name (`platform` here) — which half of the deploy owns it.
//   Repository  the source repo — REUSES the `repoUrl` config (deployment-unique → config-driven,
//               never hardcoded).
// The per-resource `Name` tag (the console's display name) CANNOT come from defaultTags — it is added
// explicitly on each key resource below. Cloudflare resources are NOT AWS-taggable: the DNS records
// carry a free-form `comment` marker instead; the tunnel + Access app/policy have no comment/tag field
// (their `name`s already read "Recollect …"). See deploy/README.md "Tagging".
const commonTags: Record<string, string> = {
  Project: "recollect",
  Environment: environment,
  ManagedBy: "pulumi",
  Stack: pulumi.getProject(),
  Repository: repoUrl,
};
// A consistent marker for the one kind of Cloudflare resource that takes a free-form comment (DNS).
const cfManagedComment = "managed by Pulumi — recollect";

// Region-scoped AWS provider so a stack's region config actually moves the box. `defaultTags` makes
// every resource this provider creates inherit `commonTags`; a per-resource `tags` only needs to add
// what's resource-specific (e.g. `Name`) and AWS merges the two.
const awsProvider = new aws.Provider("aws", { region, defaultTags: { tags: commonTags } });
const awsOpts = { provider: awsProvider };

// ---------------------------------------------------------------------------------------------
// Cloudflare — the named Tunnel, its config, and the DNS route.
// ---------------------------------------------------------------------------------------------

// The tunnel's shared secret: 32+ random bytes, base64. Pulumi-generated and stored in state
// (secret), never hand-managed. The connector token (read back below) is derived from it.
const tunnelSecret = new random.RandomBytes("tunnel-secret", { length: 40 });

const tunnel = new cloudflare.ZeroTrustTunnelCloudflared("recollect", {
  accountId: cfAccountId,
  name: "recollect",
  // We manage the tunnel's ingress declaratively below (configSrc = cloudflare).
  configSrc: "cloudflare",
  tunnelSecret: tunnelSecret.base64,
});

// The connector token the cloudflared container runs with (`tunnel run --token …`). Read back
// from the tunnel so Pulumi owns the whole lifecycle (no manual token copy/paste). It is a
// secret — wrapped so it never prints in plaintext logs/state diffs.
const tunnelToken = pulumi.secret(
  cloudflare.getZeroTrustTunnelCloudflaredTokenOutput({
    accountId: cfAccountId,
    tunnelId: tunnel.id,
  }).token,
);

// ---------------------------------------------------------------------------------------------
// Cloudflare Access (Zero Trust) — the gate in front of Grafana. The tunnel can REACH Grafana,
// but Access requires the visitor to authenticate as an allowed email BEFORE any request reaches
// the origin. So grafana.<domain> resolves and is fronted by Cloudflare, yet is NEVER publicly
// usable — only the maintainer (the allow-list below) gets in. All free tier (Access is free for
// up to 50 seats). The maintainer reaches it by visiting https://grafana.<domain>, getting the
// Cloudflare Access login (a one-time PIN to the allowed email, or an IdP if one is configured),
// and landing on Grafana once authenticated.
//
// Defined BEFORE the tunnel ingress so the optional R2-1 origin-JWT block (below) can bind the
// connector's validation to THIS app's AUD (`grafanaAccessApp.aud`).
// ---------------------------------------------------------------------------------------------

// The allow policy: a reusable, named Zero Trust policy that ALLOWS exactly the maintainer email.
// `include` is an OR of matchers; one email matcher here ⇒ only that address passes. Anyone else
// is denied by default (no other policy grants them in).
const grafanaAccessPolicy = new cloudflare.ZeroTrustAccessPolicy("grafana-maintainer", {
  accountId: cfAccountId,
  name: "Recollect Grafana — maintainer only",
  decision: "allow",
  includes: [{ email: { email: maintainerEmail } }],
});

// The self-hosted Access application bound to grafana.<domain>. It references the allow policy by
// id (precedence 1). `sessionDuration` keeps a successful login valid for a day so the maintainer
// isn't re-prompted every visit. `httpOnlyCookieAttribute` + same-site hardening on the auth cookie.
// Captured in a const so its `.aud` (the app's Access AUD tag) can feed the optional R2-1 origin-JWT
// validation on the tunnel ingress below.
const grafanaAccessApp = new cloudflare.ZeroTrustAccessApplication("grafana", {
  accountId: cfAccountId,
  name: "Recollect Grafana",
  domain: grafanaHostname,
  type: "self_hosted",
  sessionDuration: "24h",
  appLauncherVisible: true,
  httpOnlyCookieAttribute: true,
  sameSiteCookieAttribute: "lax",
  policies: [{ id: grafanaAccessPolicy.id, precedence: 1 }],
});

// R2-1 (defense-in-depth, OPTIONAL): when `cfTeamName` is configured, the grafana ingress validates
// the Cloudflare Access JWT AT THE CONNECTOR (`cloudflared`), so any L7 request reaching `lgtm:3000`
// without a valid `Cf-Access-Jwt-Assertion` for this app's AUD is rejected at the origin — even one
// that somehow bypassed the EDGE Access check. UNSET ⇒ `undefined`, i.e. the current edge-only
// posture (still Access-gated at Cloudflare; just no second origin check). Typed via the ingress
// element so it typechecks whether present or absent. See deploy/README.md "R2-1".
type GrafanaIngress = cloudflare.types.input.ZeroTrustTunnelCloudflaredConfigConfigIngress;
const grafanaOriginRequest: GrafanaIngress["originRequest"] = cfTeamName
  ? { access: { required: true, teamName: cfTeamName, audTags: [grafanaAccessApp.aud] } }
  : undefined;

// Tunnel ingress: the public hostnames → the on-box origins. TWO rules over the one tunnel:
//   1. play.<domain>     → the game server (the box's site copy + the wss socket; one origin, no
//                          CORS — the wasm play client it serves dials this same origin). The apex +
//                          `www` are the static WEBSITE on Cloudflare Pages (the block below), NOT
//                          this tunnel, so the two never collide on the one zone.
//   2. grafana.<domain>  → the self-hosted Grafana (:3000), GATED by the Cloudflare Access app
//                          above — the tunnel reaches Grafana, but Access authenticates every
//                          request at the edge first, so Grafana is never publicly reachable. When
//                          `cfTeamName` is set, `originRequest.access` (above) adds the R2-1 origin
//                          JWT check at the connector too (defense-in-depth).
// The required trailing catch-all returns 404 for anything off-hostname. Order matters: specific
// hostnames before the catch-all.
new cloudflare.ZeroTrustTunnelCloudflaredConfig("recollect", {
  accountId: cfAccountId,
  tunnelId: tunnel.id,
  config: {
    ingresses: [
      { hostname: gameHostname, service: "http://server:8080" },
      { hostname: grafanaHostname, service: "http://lgtm:3000", originRequest: grafanaOriginRequest },
      { service: "http_status:404" },
    ],
  },
});

// Proxied DNS: play.<domain> → <tunnel-id>.cfargotunnel.com. `proxied: true` puts Cloudflare
// in front (edge TLS, CDN for the static assets, WebSocket proxying for the wss socket). The apex +
// `www` records live in the Cloudflare Pages block below (→ the *.pages.dev origin), not here.
new cloudflare.DnsRecord("recollect-game", {
  zoneId: cfZoneId,
  name: gameHostname,
  type: "CNAME",
  content: pulumi.interpolate`${tunnel.id}.cfargotunnel.com`,
  ttl: 1, // 1 = "automatic"; required while proxied.
  proxied: true,
  // Cloudflare resources can't carry the AWS tag set, but a DNS record takes a free-form comment —
  // the closest equivalent provenance marker (visible in the dashboard's DNS table).
  comment: cfManagedComment,
});

// Proxied DNS for the Grafana subdomain → the same tunnel. Same edge (TLS + proxy); the tunnel's
// grafana.<domain> ingress rule (above) sends it to the on-box Grafana. The Access app (above)
// guards it.
new cloudflare.DnsRecord("recollect-grafana", {
  zoneId: cfZoneId,
  name: grafanaHostname,
  type: "CNAME",
  content: pulumi.interpolate`${tunnel.id}.cfargotunnel.com`,
  ttl: 1,
  proxied: true,
  comment: cfManagedComment,
});

// ---------------------------------------------------------------------------------------------
// Cloudflare Pages — the static WEBSITE (apex + www), all as IaC. The marketing/rules/cards/lore/
// guide bundle (`make site` → dist/) is a pure static site, deployed SEPARATELY from this box: CI
// (.github/workflows/site-deploy.yml) `wrangler pages deploy`s the built dist/ straight to the Pages
// project below — a DIRECT upload, so Cloudflare never reads the (private) repo and no git connection
// exists. This block makes the parts that used to be manual dashboard clicks reproducible:
//   • the Pages PROJECT (direct-upload; productionBranch=main; NO `source` ⇒ no git integration),
//   • the two custom-domain BINDINGS (apex + www) that tell Pages to serve those hostnames + mint
//     their edge certs, and
//   • the two zone DNS CNAMEs (apex + www → the project's <name>.pages.dev), proxied for edge TLS+CDN.
// What still CAN'T be IaC (a token can't mint itself): the Pages:Edit CLOUDFLARE_API_TOKEN + the
// GitHub secrets/var CI deploys with — see deploy/site/README.md's residual manual steps.
// ---------------------------------------------------------------------------------------------

// The Pages project. OMITTING `source` is what makes it a DIRECT-UPLOAD project (no connected Git
// repo) — exactly the intended state: CI builds + uploads dist/, Cloudflare just receives it. The
// `pagesProjectName` (a generic default, overridable) is exported below so CI's CF_PAGES_PROJECT var
// can name the SAME project Pulumi created. Pages resources take no AWS tags + have no comment field.
const pagesProject = new cloudflare.PagesProject("recollect-site", {
  accountId: cfAccountId,
  name: pagesName,
  productionBranch: "main",
});

// The custom-domain bindings: apex + www, attached to the project. A PagesDomain tells Pages to
// SERVE that hostname (and provision its edge certificate); it is SEPARATE from the zone DNS record
// (next) — Cloudflare requires both. Same-zone domains validate automatically (the zone is already on
// Cloudflare). www is conventional + recommended; both land on the same static site.
const wwwHostname = `www.${domain}`;
new cloudflare.PagesDomain("recollect-site-apex", {
  accountId: cfAccountId,
  projectName: pagesProject.name,
  name: domain,
});
new cloudflare.PagesDomain("recollect-site-www", {
  accountId: cfAccountId,
  projectName: pagesProject.name,
  name: wwwHostname,
});

// The zone DNS for the website: apex + www → the project's <name>.pages.dev origin. `proxied: true`
// fronts them with Cloudflare (edge TLS + the CDN). At the apex this is CNAME flattening, which
// Cloudflare handles transparently. `pagesProject.subdomain` is the project's *.pages.dev hostname
// (an output of the resource above), so the records always track the real target with no hardcoding.
new cloudflare.DnsRecord("recollect-site-apex", {
  zoneId: cfZoneId,
  name: domain,
  type: "CNAME",
  content: pagesProject.subdomain,
  ttl: 1, // 1 = "automatic"; required while proxied.
  proxied: true,
  comment: cfManagedComment,
});
new cloudflare.DnsRecord("recollect-site-www", {
  zoneId: cfZoneId,
  name: wwwHostname,
  type: "CNAME",
  content: pagesProject.subdomain,
  ttl: 1,
  proxied: true,
  comment: cfManagedComment,
});

// ---------------------------------------------------------------------------------------------
// AWS — the EC2 box, its keyless-admin role, the egress-only SG, and cloud-init.
// ---------------------------------------------------------------------------------------------

// Latest Amazon Linux 2023 AMI for the chosen arch (t3 = x86_64). Looked up so the box tracks
// security patches at provision time rather than pinning a stale AMI id.
const ami = aws.ec2.getAmiOutput(
  {
    owners: ["amazon"],
    mostRecent: true,
    filters: [
      { name: "name", values: ["al2023-ami-2023.*-x86_64"] },
      { name: "virtualization-type", values: ["hvm"] },
      { name: "state", values: ["available"] },
    ],
  },
  awsOpts,
);

// Keyless admin via SSM Session Manager: an instance role with the managed SSM policy, so the
// operator opens a shell with `aws ssm start-session` — no SSH key, no inbound 22. (To add SSH
// instead, attach a key pair + a port-22 ingress rule; the tunnel means neither is needed.)
const role = new aws.iam.Role(
  "recollect-ec2",
  {
    assumeRolePolicy: JSON.stringify({
      Version: "2012-10-17",
      Statement: [
        {
          Action: "sts:AssumeRole",
          Effect: "Allow",
          Principal: { Service: "ec2.amazonaws.com" },
        },
      ],
    }),
    // common tags via defaultTags; only the console display Name is per-resource.
    tags: { Name: "recollect-instance-role" },
  },
  awsOpts,
);
new aws.iam.RolePolicyAttachment(
  "recollect-ssm",
  { role: role.name, policyArn: "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore" },
  awsOpts,
);
// The CloudWatch agent on the box publishes the custom host metrics (mem/swap/disk) the §11
// out-of-band alarms watch. This is AWS's RECOMMENDED managed policy for the agent: it grants
// `cloudwatch:PutMetricData` + a few `ec2:Describe*` + scoped `ssm:GetParameter` on
// `AmazonCloudWatch-*` (the agent config) — all this box actually uses. It ALSO carries `logs:*` and
// X-Ray write (Resource:*), which this deploy never exercises (no log shipping — logs live in on-box
// Loki). A tighter customer-managed policy could drop those, but the managed policy is the blessed,
// low-maintenance default for a launch host; the unused grants are write-only to this account's own
// CloudWatch/X-Ray, not a lateral-movement surface.
new aws.iam.RolePolicyAttachment(
  "recollect-cwagent",
  { role: role.name, policyArn: "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy" },
  awsOpts,
);
// ECR READ-ONLY: the box pulls the server image FOUNDATION's CI pushed (production no longer builds
// on-box). AWS's managed `AmazonEC2ContainerRegistryReadOnly` is the standard keyless pull grant —
// `ecr:GetAuthorizationToken` (the registry login) + the BatchGet/GetDownloadUrl/DescribeImages READ
// set, and NOTHING that writes or deletes. So cloud-init's `aws ecr get-login-password | docker login`
// + `docker compose pull` work with no stored credentials (the instance role IS the credential), and
// the box can never push or mutate a repo. (Read is account-wide in the managed policy; the box only
// ever pulls the one server repo — the WRITE surface is what matters, and there is none.)
new aws.iam.RolePolicyAttachment(
  "recollect-ecr-pull",
  { role: role.name, policyArn: "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly" },
  awsOpts,
);
const instanceProfile = new aws.iam.InstanceProfile(
  "recollect-ec2",
  { role: role.name, tags: { Name: "recollect-instance-profile" } },
  awsOpts,
);

// Default VPC + a subnet to place the box (the free-tier story doesn't need a bespoke VPC).
const defaultVpc = aws.ec2.getVpcOutput({ default: true }, awsOpts);

// EGRESS-ONLY security group: the Cloudflare Tunnel dials OUT (443 to Cloudflare), so the box
// needs NO inbound rule at all. Egress is left wide so apt/docker/cloudflared/Postgres can reach
// what they need; ingress is empty (the strongest posture — nothing on the public internet can
// open a socket to the box).
const sg = new aws.ec2.SecurityGroup(
  "recollect",
  {
    description: "Recollect launch host - egress only; ingress via Cloudflare Tunnel (no inbound).",
    vpcId: defaultVpc.id,
    egress: [
      {
        protocol: "-1",
        fromPort: 0,
        toPort: 0,
        cidrBlocks: ["0.0.0.0/0"],
        ipv6CidrBlocks: ["::/0"],
        description: "all egress (tunnel dials out; package + image pulls)",
      },
    ],
    // common tags via defaultTags; only the console display Name is per-resource.
    tags: { Name: "recollect-sg" },
  },
  awsOpts,
);

// On-box Postgres password: GENERATED, never an operator input. Postgres lives only on the
// compose network (no published port — see docker-compose.deploy.yml), so its password is purely
// an internal DSN credential; Pulumi mints it and is its sole keeper. 40 URL-safe alphanumerics
// (`special: false`) so it drops into the `postgres://…` DSN with no percent-encoding hazard.
// `.result` is a secret output (the provider marks it so), so it stays encrypted in state + the
// rendered user-data and never prints in plaintext. The operator never sees or supplies it; if
// `docker exec psql` ever needs it, it is in the box's 0600 `.env` or `pulumi stack output
// postgresPassword --show-secrets`.
const pgPassword = new random.RandomPassword("postgres-password", {
  length: 40,
  special: false,
}).result;

// Render the cloud-init script: read the committed template and substitute the @@PLACEHOLDERS@@.
// Done with pulumi.all so the secret token/password stay tracked as secrets in state + outputs.
const userDataTemplate = fs.readFileSync(path.join(__dirname, "user-data.sh"), "utf8");
const renderedUserData = pulumi
  .all([tunnelToken, pgPassword, cfBeaconToken])
  .apply(([token, pgPass, beacon]) =>
    userDataTemplate
      .replace(/@@REPO_URL@@/g, repoUrl)
      .replace(/@@GIT_REF@@/g, gitRef)
      // The ECR server image the box PULLS (FOUNDATION/PLATFORM split): cloud-init writes it as
      // IMAGE_REF in .env, logs in to ECR with the instance role, and `compose pull`s it — no on-box
      // Rust build. (The repo is still cloned for the compose files + the site build.)
      .replace(/@@IMAGE_REF@@/g, serverImage)
      // The play client the BOX serves is reached at the game origin (play.<domain>) and dials it
      // same-origin for the wss; SITE_ORIGIN is the human label for that origin. (The apex/www site
      // lives on Cloudflare Pages, a separate origin.)
      .replace(/@@SITE_ORIGIN@@/g, `https://${gameHostname}`)
      .replace(/@@TUNNEL_TOKEN@@/g, token)
      .replace(/@@POSTGRES_PASSWORD@@/g, pgPass)
      .replace(/@@OTEL_ENDPOINT@@/g, otelEndpoint)
      .replace(/@@CF_BEACON_TOKEN@@/g, beacon)
      .replace(/@@SWAP_SIZE_GB@@/g, String(swapSizeGb))
      // The bare domain the compose builds the Grafana root URL from (https://grafana.<domain>).
      .replace(/@@OBS_GRAFANA_DOMAIN@@/g, domain),
  );

// EC2 caps user_data at 16 KiB (raw, pre-base64) and the AWS provider validates it at PLAN time.
// The rendered script is ~17.4 KiB, so we ship it gzip-COMPRESSED via userDataBase64: cloud-init on
// Amazon Linux 2023 detects the gzip magic bytes and decompresses before running it, which drops the
// payload to ~7 KiB (well under the cap, with room for the script to grow). We normalize the gzip
// header — zero the MTIME and set the OS byte to 0xFF ("unknown") — so the bytes are IDENTICAL across
// whatever machine runs `pulumi up`; otherwise userDataReplaceOnChange would spuriously replace the
// box on a deploy from a different host. Deriving via .apply keeps it a tracked SECRET in state.
const userDataBase64 = renderedUserData.apply((script) => {
  const gz = zlib.gzipSync(Buffer.from(script, "utf8"), {
    level: zlib.constants.Z_BEST_COMPRESSION,
  });
  gz.writeUInt32LE(0, 4); // MTIME → 0 (Node's default, made explicit)
  gz[9] = 0xff; // OS → unknown, so the header doesn't vary by build platform
  return gz.toString("base64");
});

const instance = new aws.ec2.Instance(
  "recollect",
  {
    ami: ami.id,
    instanceType,
    iamInstanceProfile: instanceProfile.name,
    vpcSecurityGroupIds: [sg.id],
    // No subnetId ⇒ AWS places the box in the default VPC's default subnet. It gets a public IP
    // (the default subnet's free egress path for apt/docker/cloudflared pulls), but the SG has
    // ZERO inbound rules, so that IP is unreachable — the only way in is the outbound tunnel.
    // Root volume shrunk to rootVolumeSizeGb (default 10 GiB) — the #31 cost fix: root (10) +
    // the durable /data volume (20) = 30 GiB, the WHOLE 30 GB/12-month EBS free tier, so storage
    // is $0 during the window (the durable data lives on /data, so the small root is fine).
    rootBlockDevice: { volumeSize: rootVolumeSizeGb, volumeType: "gp3", encrypted: true },
    // gzip-compressed (cloud-init decompresses on the box) to fit EC2's 16 KiB user_data cap — see
    // the userDataBase64 derivation above.
    userDataBase64,
    // Re-render + replace the box when the bootstrap inputs change (e.g. a new pinned gitRef).
    userDataReplaceOnChange: true,
    // IMDSv2 required (tokens), hop limit 1 — block SSRF-to-metadata.
    metadataOptions: { httpTokens: "required", httpPutResponseHopLimit: 1 },
    // common tags via defaultTags; only the console display Name is per-resource.
    tags: { Name: "recollect-server" },
  },
  awsOpts,
);

// ---------------------------------------------------------------------------------------------
// AWS — the DURABLE data volume (state that must survive the box: Postgres now, observability soon).
// ---------------------------------------------------------------------------------------------

// The crux of durability: the box's stateful data lives on a SEPARATE encrypted EBS volume, NOT the
// instance root. user-data mounts it at /data, and each stateful service gets its own subdir
// (/data/postgres today; a light self-hosted Grafana + metrics TSDB will share /data soon). So when
// `pulumi up` replaces the box (e.g. a new pinned gitRef ⇒ userDataReplaceOnChange) or the instance
// is terminated, the root goes away but THIS volume does not — it detaches from the old instance and
// re-attaches to the new one, and user-data MOUNTS (never reformats) it. The match journal +
// accounts live on. A standalone `aws.ebs.Volume` (vs. an instance `ebsBlockDevice`) is deliberate:
// a block device defined on the instance is destroyed with the instance; a separate volume has its
// own lifecycle and is NEVER deleted on instance termination.
//
// It MUST sit in the same AZ as the instance (EBS is AZ-local) — `instance.availabilityZone` ties
// the two together so they always match. gp3 + encrypted mirrors the root's posture. The
// `recollect:data` tag marks it as the durable data volume (e.g. for a replacement box to match).
//
// Why it survives a box REPLACE without any extra protection: replacing the instance replaces only
// the VolumeAttachment below (its `instanceId` input changes) — the Volume's own inputs don't change
// on a gitRef bump, so Pulumi never replaces the Volume; it stays put and the new box's attachment
// re-binds it. (We deliberately do NOT set `protect`/`retainOnDelete`: a real `pulumi destroy` should
// still cleanly delete the volume rather than wedge teardown or orphan a billable resource. Recovery
// from accidents is `make db-backup` + the volume simply outliving the instance, not snapshots.)
const dataVolume = new aws.ebs.Volume(
  "recollect-data",
  {
    availabilityZone: instance.availabilityZone,
    size: dataVolumeSizeGb,
    type: "gp3",
    encrypted: true,
    // Project/Environment/ManagedBy/Stack/Repository come from the provider's defaultTags. `Name` is
    // the console display name; `recollect:data` marks it as THE durable data volume (a replacement
    // box matches on it).
    tags: { Name: "recollect-data-volume", "recollect:data": "true" },
  },
  awsOpts,
);

// Attach the data volume at /dev/sdf. On a Nitro instance (t3) the kernel exposes this as an NVMe
// device (/dev/nvme1n1 etc.), which user-data resolves to from the stable /dev/sdf identifier.
// `deleteOnTermination` is left at its default (FALSE) — a separate volume's attachment never tears
// the volume down — which is exactly the durability we want.
new aws.ec2.VolumeAttachment(
  "recollect-data",
  {
    deviceName: "/dev/sdf",
    volumeId: dataVolume.id,
    instanceId: instance.id,
  },
  awsOpts,
);

// ---------------------------------------------------------------------------------------------
// AWS Budgets — free-tier guardrails with email alerts (so a mistake can't run up a bill).
// ---------------------------------------------------------------------------------------------

// Notify at 80% actual and 100% forecast of the monthly cap. With budgetEmail set, both email.
const notifications: aws.types.input.budgets.BudgetNotification[] = budgetEmail
  ? [
      {
        comparisonOperator: "GREATER_THAN",
        threshold: 80,
        thresholdType: "PERCENTAGE",
        notificationType: "ACTUAL",
        subscriberEmailAddresses: [budgetEmail],
      },
      {
        comparisonOperator: "GREATER_THAN",
        threshold: 100,
        thresholdType: "PERCENTAGE",
        notificationType: "FORECASTED",
        subscriberEmailAddresses: [budgetEmail],
      },
    ]
  : [];

new aws.budgets.Budget(
  "recollect-monthly",
  {
    budgetType: "COST",
    timeUnit: "MONTHLY",
    limitAmount: monthlyBudgetUsd,
    limitUnit: "USD",
    notifications,
    tags: { Name: "recollect-monthly-budget" },
  },
  awsOpts,
);

// A dedicated low-dollar guardrail: the moment ACTUAL monthly spend crosses $1 (you've left $0
// free-tier territory — a paid instance size, an egress overrun, …), this fires. A tighter,
// actual-cost tripwire than the FORECASTED monthly budget above. No cost filter: AWS Budgets has no
// "RecordType" dimension, and tracking total actual cost is what we want here — credits that zero the
// bill keep it quiet, so it fires only when you are genuinely being charged.
new aws.budgets.Budget(
  "recollect-free-tier-guard",
  {
    budgetType: "COST",
    timeUnit: "MONTHLY",
    limitAmount: "1",
    limitUnit: "USD",
    notifications: budgetEmail
      ? [
          {
            comparisonOperator: "GREATER_THAN",
            threshold: 1,
            thresholdType: "ABSOLUTE_VALUE",
            notificationType: "ACTUAL",
            subscriberEmailAddresses: [budgetEmail],
          },
        ]
      : [],
    tags: { Name: "recollect-free-tier-guard-budget" },
  },
  awsOpts,
);

// ---------------------------------------------------------------------------------------------
// CloudWatch — the OUT-OF-BAND box-health net (tech-design §11). This is the SECOND, independent
// eye: the in-box Grafana/node-exporter dashboard can't alarm on its OWN outage (if the box wedges,
// so does Grafana), so a handful of CloudWatch alarms — on AWS-native EC2 metrics + the custom
// host metrics the on-box CloudWatch agent publishes — page the maintainer by email even then.
// FREE-TIER DISCIPLINE: 7 alarms (≤10 free), 4 custom metrics (≤10 free), 5-minute basic metrics,
// NO detailed monitoring. SNS email notifications are free for the volumes here.
// ---------------------------------------------------------------------------------------------

// The SNS topic the alarms publish to, with an email subscription (when alarmEmail is set). The
// subscriber must click the confirmation email once before alerts deliver.
const alarmTopic = new aws.sns.Topic(
  "recollect-alarms",
  { name: "recollect-alarms", tags: { Name: "recollect-alarms" } },
  awsOpts,
);
if (alarmEmail) {
  new aws.sns.TopicSubscription(
    "recollect-alarms-email",
    { topic: alarmTopic.arn, protocol: "email", endpoint: alarmEmail },
    awsOpts,
  );
}

// Common alarm wiring: notify the SNS topic on ALARM, and treat "missing data" as a problem for the
// system status check (a box that stopped reporting is itself the alert) but as "ignore" for the
// custom host metrics (a brief agent gap shouldn't false-alarm).
const dims = instance.id.apply((id) => ({ InstanceId: id }));
const alarmActions = [alarmTopic.arn];

// 1. EC2 instance status check failed — the guest/OS is unhealthy (the instance-level check).
new aws.cloudwatch.MetricAlarm(
  "recollect-status-instance",
  {
    alarmDescription: "EC2 instance status check failed (OS/guest unhealthy).",
    namespace: "AWS/EC2",
    metricName: "StatusCheckFailed_Instance",
    dimensions: dims,
    statistic: "Maximum",
    period: 300,
    evaluationPeriods: 2,
    threshold: 1,
    comparisonOperator: "GreaterThanOrEqualToThreshold",
    treatMissingData: "missing",
    alarmActions,
    okActions: alarmActions,
    tags: { Name: "recollect-status-instance" },
  },
  awsOpts,
);

// 2. EC2 SYSTEM status check failed — the underlying host/network is unhealthy. Add the free
// built-in EC2 `recover` action so AWS auto-recovers the instance onto healthy hardware.
new aws.cloudwatch.MetricAlarm(
  "recollect-status-system",
  {
    alarmDescription: "EC2 system status check failed (underlying host unhealthy) - auto-recovers.",
    namespace: "AWS/EC2",
    metricName: "StatusCheckFailed_System",
    dimensions: dims,
    statistic: "Maximum",
    period: 300,
    evaluationPeriods: 2,
    threshold: 1,
    comparisonOperator: "GreaterThanOrEqualToThreshold",
    treatMissingData: "missing",
    alarmActions: [alarmTopic.arn, `arn:aws:automate:${region}:ec2:recover`],
    okActions: alarmActions,
    tags: { Name: "recollect-status-system" },
  },
  awsOpts,
);

// 3. CPUUtilization sustained high — on a t3.micro this also burns CPU credits; the box idles low,
// so a sustained pin is a real signal (a hot loop, a build that never ended, or genuine load).
new aws.cloudwatch.MetricAlarm(
  "recollect-cpu-high",
  {
    alarmDescription: `EC2 CPUUtilization >= ${cpuAlarmThresholdPct}% sustained.`,
    namespace: "AWS/EC2",
    metricName: "CPUUtilization",
    dimensions: dims,
    statistic: "Average",
    period: 300,
    evaluationPeriods: 3,
    threshold: cpuAlarmThresholdPct,
    comparisonOperator: "GreaterThanOrEqualToThreshold",
    treatMissingData: "missing",
    alarmActions,
    okActions: alarmActions,
    tags: { Name: "recollect-cpu-high" },
  },
  awsOpts,
);

// 4. Memory used high — the custom mem_used_percent from the CloudWatch agent. The 1 GB box is the
// real risk; this is the canary for "swap is about to thrash / OOM-killer territory".
new aws.cloudwatch.MetricAlarm(
  "recollect-mem-high",
  {
    alarmDescription: "Host memory used >= 90% (custom CloudWatch-agent metric).",
    namespace: "Recollect/Host",
    metricName: "mem_used_percent",
    dimensions: dims,
    statistic: "Average",
    period: 300,
    evaluationPeriods: 3,
    threshold: 90,
    comparisonOperator: "GreaterThanOrEqualToThreshold",
    treatMissingData: "notBreaching",
    alarmActions,
    okActions: alarmActions,
    tags: { Name: "recollect-mem-high" },
  },
  awsOpts,
);

// 5. Swap used high — the box leans on the 4 GB swap file; sustained high swap means real memory
// pressure (consider t3.small). The companion to the mem alarm.
new aws.cloudwatch.MetricAlarm(
  "recollect-swap-high",
  {
    alarmDescription: "Host swap used >= 70% (custom CloudWatch-agent metric).",
    namespace: "Recollect/Host",
    metricName: "swap_used_percent",
    dimensions: dims,
    statistic: "Average",
    period: 300,
    evaluationPeriods: 3,
    threshold: 70,
    comparisonOperator: "GreaterThanOrEqualToThreshold",
    treatMissingData: "notBreaching",
    alarmActions,
    okActions: alarmActions,
    tags: { Name: "recollect-swap-high" },
  },
  awsOpts,
);

// 6 + 7. Disk space high on the two real mounts. `/` filling = Docker images/logs; `/data` filling
// = Postgres + the observability stores (retention should bound it — this is the backstop). The
// CloudWatch agent tags disk_used_percent with `path`, so each mount is its own alarm.
for (const [name, path] of [
  ["recollect-disk-root", "/"],
  ["recollect-disk-data", "/data"],
] as const) {
  new aws.cloudwatch.MetricAlarm(
    name,
    {
      alarmDescription: `Host disk used >= 85% on ${path} (custom CloudWatch-agent metric).`,
      namespace: "Recollect/Host",
      metricName: "disk_used_percent",
      dimensions: instance.id.apply((id) => ({ InstanceId: id, path })),
      statistic: "Average",
      period: 300,
      evaluationPeriods: 2,
      threshold: 85,
      comparisonOperator: "GreaterThanOrEqualToThreshold",
      treatMissingData: "notBreaching",
      alarmActions,
      okActions: alarmActions,
      tags: { Name: name },
    },
    awsOpts,
  );
}

// ---------------------------------------------------------------------------------------------
// Outputs — what the operator needs after `pulumi up`.
// ---------------------------------------------------------------------------------------------
export const instanceId = instance.id;
// The durable data volume's id — it persists across box recreation; surfaced so the operator can
// confirm the SAME volume re-attached after a `pulumi up` that replaced the instance, and to find
// it in the console.
export const dataVolumeId = dataVolume.id;
export const tunnelId = tunnel.id;
// The WEBSITE — the apex, served by Cloudflare Pages (the static marketing/rules/cards/lore/guide
// bundle). `www.<domain>` resolves to the same Pages site.
export const site = pulumi.interpolate`https://${domain}`;
// The GAME SERVER origin (play.<domain>) — the box behind the tunnel; the wss socket + REST + the
// box's play-client copy. The website's "Play" link points here.
export const gameUrl = pulumi.interpolate`https://${gameHostname}`;
// The Cloudflare Pages PROJECT NAME Pulumi created (direct-upload). Wire CI's CF_PAGES_PROJECT GitHub
// var to THIS so `wrangler pages deploy --project-name` targets the same project. (Exported from the
// resource's own `.name` so the output reflects what Pulumi actually created.) See
// deploy/site/README.md.
export const pagesProjectName = pagesProject.name;
// The project's *.pages.dev hostname — the origin the apex/www CNAMEs point at (handy for debugging
// DNS / confirming the binding).
export const pagesSubdomain = pagesProject.subdomain;
// The Access-gated Grafana URL — the maintainer (the allow-listed email) visits this, authenticates
// via Cloudflare Access, and lands on the self-hosted dashboards. Never publicly reachable.
export const grafanaUrl = pulumi.interpolate`https://${grafanaHostname}`;
// The SNS topic the out-of-band CloudWatch box-health alarms publish to.
export const alarmTopicArn = alarmTopic.arn;
export const ssmSession = pulumi.interpolate`aws ssm start-session --region ${region} --target ${instance.id}`;
// The connector token (already a secret) — surfaced encrypted for debugging a tunnel that won't
// connect: `pulumi stack output cloudflaredToken --show-secrets`.
export const cloudflaredToken = tunnelToken;
// The generated on-box Postgres password (a secret) — surfaced encrypted for the rare
// `docker exec … psql` on the box: `pulumi stack output postgresPassword --show-secrets`.
// (It also lives in the box's 0600 root-only `.env`.)
export const postgresPassword = pgPassword;
