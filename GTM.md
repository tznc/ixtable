# ixtable go-to-market inventory

Status of everything ixtable needs to reach paying customers, and who does
each item. It complements `PRD.md`: the PRD defines the product and its
launch gates, this file tracks the work around it.

Status is Done, Partial, Missing, or Deferred. Owner is one of:

- You: needs an account, money, a legal decision, or your judgment.
- Claude: code or content Claude can write in these repos.
- Both: Claude prepares it, you approve it or supply a value.

## Decisions already made

- Business model: free Apache-2.0 desktop app, paid ixtable Cloud priced
  per cloud application with a runtime-user allowance (PRD §4).
- Privacy: the desktop app and Cloud send no third-party analytics.
  Activation is measured only from Cloud sign-ups, as daily aggregate counts.
  GA4 runs on the marketing website only (PRD §27.5, §30).
- Payments: Stripe direct with Stripe Tax.
- Pricing: three plans per cloud application, billed monthly with no free
  trial. Starter is US$19 with 5 runtime users, Team US$49 with 25, and
  Business US$149 with 100. The catalog lives in `public.plans`, with a static
  copy in `web/src/data/plans.json`. There is no annual billing.
- Website CTAs: waitlist (primary), GitHub, download marked "coming soon"
  until signed installers exist.
- Measurement: biz-tools collects daily aggregates into its repo, writes a
  monthly report, and opens `gtm-alert` issues in biz-tools.

## Inventory

### 1. Positioning and messaging

| Item | Status | Owner | Gap |
|---|---|---|---|
| Ideal customer profile | Partial | You | PRD §3.1 names technical operations teams. No list of target industries, team sizes, or the spreadsheets/Access databases they replace. |
| Positioning statement | Missing | Both | One sentence against Access, Airtable, NocoDB, and Retool. Strongest wedge: Access does not run on macOS or Linux; ixtable does, and works offline. |
| Landing page copy | Partial | Claude | Hero and features exist. Needs the positioning statement and proof (screenshots of a golden app). |
| Comparison pages | Missing | Claude | "Microsoft Access alternative", "Access for Mac", "ixtable vs NocoDB / Baserow / Budibase / Retool". The monthly competitor-gap report feeds these. |
| Customer interviews | Missing | You | 10 to 15 conversations with target users to validate the positioning and the price. |

### 2. Pricing and packaging

| Item | Status | Owner | Gap |
|---|---|---|---|
| Pricing page | Done | Claude | Reads the plan catalog: Starter, Team, and Business, monthly only. |
| Price validation | Missing | You | Test the Starter, Team, and Business prices with interviews or a waitlist survey before launch. |
| Over-allowance behaviour | Done | Claude | Access past the plan's runtime-user allowance is refused (`over_allowance`). The owner upgrades the plan. |
| Trial | Done | You | No free trial. The desktop app is the free tier. |
| Discounts | Missing | You | Education, non-profit, and open-source contributor policy. |

### 3. Product readiness (PRD launch gates)

| Item | Status | Owner | Gap |
|---|---|---|---|
| PRD Phases 0 to 5 | Partial | Both | Phase 5 (§28) is the commercial gate. Track it separately from this file. |
| Signed macOS installer | Missing | You | Apple Developer Program (US$99/year), Developer ID certificate, notarization credentials in CI. |
| Signed Windows installer | Missing | You | Code-signing certificate, or Azure Trusted Signing. |
| Linux packages | Missing | Claude | AppImage and .deb from Tauri, published to GitHub Releases. |
| Desktop updates | Deferred | You | No in-app updater before MVP (PRD §28). Users download new versions from GitHub Releases. |
| Onboarding with golden apps | Missing | Claude | PRD §26: CRM, inventory, work orders as first-run templates. |
| Opt-in crash reports | Missing | Claude | PRD §27.5 requires informed consent. Self-hosted endpoint or a provider you choose. |
| Docs accuracy | Partial | Claude | `docs/concepts/deployment.md` says deployment is undefined; it should describe PRD §21 to §23 as planned. |

### 4. Website (ixtable.com)

| Item | Status | Owner | Gap |
|---|---|---|---|
| Domain and canonical URLs | Partial | Claude | Sitemap, canonical links, and `robots.txt` use ixtable.com. Canonical URLs have no trailing slash, so Cloudflare Pages answers each with a 308 to the slash form. Emitting slash URLs needs the invite, desktop sign-in, and checkout links to change first, because `docusaurus serve` drops the query string on its slash redirect. |
| Redirect from ixtable.pages.dev | Missing | You | Cloudflare redirect rule, 301 to ixtable.com. |
| GA4 | Partial | You | Code is in place; set the `GA4_MEASUREMENT_ID` repository variable. |
| Cookie consent for GA4 | Missing | Both | GA4 needs consent for EU and UK visitors. Pick a consent tool or use Google Consent Mode v2 with a banner; I wire it. |
| Waitlist | Partial | You | Form and table exist. Needs a hosted Supabase project to receive sign-ups. |
| Pricing page | Done | Claude | Live plan catalog, monthly only. |
| Blog | Done | Claude | Enabled with a first post; weekly drafts arrive as PRs. |
| Legal pages | Partial | Both | Terms and privacy policy exist and await legal review. Cookie policy and subprocessors list are missing. |
| Security page | Done | Claude | `docs/cloud/security.md` documents the trusted-user security model. |
| Changelog and roadmap | Missing | Claude | Public changelog from releases; roadmap from PRD phases. |
| Status page | Missing | You | Hosted status page for Cloud once it exists. |

### 5. Accounts and email

| Item | Status | Owner | Gap |
|---|---|---|---|
| Hosted Supabase project | Missing | You | Create it, apply the migrations in `supabase/migrations`, set `SUPABASE_URL` and `SUPABASE_ANON_KEY` for the site build. |
| Auth URLs | Missing | You | Set `site_url` and redirect URLs to ixtable.com in the hosted project. |
| Custom SMTP | Missing | You | Supabase's built-in email is rate-limited and not meant for production. Use Resend or Postmark with SPF, DKIM, and DMARC on ixtable.com. |
| Google and Microsoft sign-in | Missing | You | PRD §21.1 requires both. The local config has GitHub and Google placeholders; Microsoft (Azure) needs adding. |
| Email domain | Missing | You | support@, security@, and hello@ixtable.com mailboxes. |
| Waitlist launch email | Missing | Both | A sending tool with unsubscribe handling. I can build a send script on the email provider's API. |

### 6. Billing

| Item | Status | Owner | Gap |
|---|---|---|---|
| Stripe account | Missing | You | Create it, verify the business, enable Stripe Tax and register where required. |
| Products and prices | Missing | Both | One product per plan; prices in USD. I can script creation once the account exists. |
| Checkout and customer portal | Done | Claude | `billing-checkout` and `billing-portal` Edge Functions. |
| Webhooks to entitlements | Done | Claude | The `stripe-webhook` Edge Function updates subscriptions. |
| `gtm_entitlements()` RPC | Missing | Claude | Returns `(subscription_id, state)` for the daily billing reconciliation in biz-tools. |
| Dunning and refunds | Missing | You | Retry schedule, failed-payment emails, refund policy. |
| Restricted Stripe key for biz-tools | Missing | You | Read-only key in the `commerce-production` environment; then set `commerce.stripe.enabled: true`. |

### 7. Legal and company

| Item | Status | Owner | Gap |
|---|---|---|---|
| Legal entity | Missing | You | Needed for Stripe, Apple, and contracts. |
| Terms of service | Missing | Both | I draft from the PRD's trusted-user model; a lawyer reviews. |
| Privacy policy | Missing | Both | Must say the desktop app collects nothing, what Cloud stores, and that the website uses GA4. |
| Data processing agreement | Missing | Both | Business customers will ask for one. |
| Subprocessors list | Missing | Claude | Supabase, Stripe, Cloudflare, Google Analytics, the email provider, S3. |
| Trademark | Missing | You | Search and register "ixtable". Apache-2.0 does not grant trademark rights, so a trademark policy stops forks using the name. |
| Contributor terms | Missing | Both | DCO sign-off or a CLA before accepting outside code. |
| `SECURITY.md` and disclosure address | Missing | Claude | Plus the independent security review PRD §21.3 requires before launch (you commission it). |

### 8. Measurement (biz-tools)

| Item | Status | Owner | Gap |
|---|---|---|---|
| Collectors and monthly report | Done | Claude | GA4, Search Console, Bing, GitHub, Cloud funnel, Stripe subscriptions, entitlements. |
| Search Console | Missing | You | Verify `ixtable.com` as a domain property; add the biz-tools service account as a user; enable in `config.yaml`. |
| Bing Webmaster | Missing | You | Add the site, then enable. |
| GA4 data | Missing | You | Give the service account Viewer access; put the numeric property ID in `config.yaml`. |
| Cloud funnel | Missing | You | After the hosted project exists: add `IXTABLE_SUPABASE_SERVICE_ROLE_KEY` to the `product-production` environment, set `cloud.url` and `cloud.enabled: true`. |
| GitHub stats | Missing | You | Enable once the repo is public; add `GTM_GITHUB_TOKEN`. |
| Monthly spend and hours | Missing | You | Add `data/ixtable/manual/<YYYY-MM>.json` each month. |
| Marketing budget | Missing | You | `business.monthlyMarketingBudget` is 0 in `config.yaml`. |
| Experiments | Missing | Both | Log one each time a change ships to the funnel. |

### 9. Channels and launch

| Item | Status | Owner | Gap |
|---|---|---|---|
| Public GitHub repository | Missing | You | The repo is private. For an Apache-2.0 product, the public repo is a primary channel. Make it public once the README and license are ready. |
| Repository README | Partial | Claude | Needs screenshots, a short demo GIF, and install steps once installers exist. |
| Search content | Partial | Both | Weekly draft PRs from keyword and competitor data; you edit and merge. |
| Directories | Missing | Both | AlternativeTo (as a Microsoft Access alternative), awesome-tauri, open-source alternative lists. I draft listings; you submit. |
| Communities | Missing | You | r/MSAccess, r/selfhosted, r/sysadmin, Hacker News (Show HN), Product Hunt, Indie Hackers. Post when installers are signed. |
| Demo video | Missing | You | 2-minute walk-through building a golden app. |
| Social accounts | Missing | You | Reserve the ixtable handle on X, Bluesky, LinkedIn, YouTube. |
| Design partners | Missing | You | 5 to 10 teams on a private pilot before public launch (PRD §33: not to be labelled as public readiness). |

### 10. Support and operations

| Item | Status | Owner | Gap |
|---|---|---|---|
| Support channel | Missing | You | GitHub Discussions for the free product; email for Cloud customers. |
| Admin and support tooling | Missing | Claude | PRD Phase 5: diagnose distribution and key-grant failures without seeing secrets. |
| Monitoring and incidents | Missing | Both | Alerts for Cloud health (PRD §27.5) and a short incident runbook. |
| Backups | Missing | Both | Supabase point-in-time recovery plan and S3 archive retention (PRD §23). |

## Actionables

### You, now (unblocks the measurement loop)

1. Verify `ixtable.com` in Search Console and add the biz-tools service account.
2. Create the GA4 property; set the `GA4_MEASUREMENT_ID` variable in ixtable; give the service account Viewer access and send me the property ID.
3. Add `ixtable.pages.dev` to `ixtable.com` as a 301 in Cloudflare.
4. Create the hosted Supabase project; send me the project URL. Add the service role key to biz-tools' `product-production` environment.
5. Set up custom SMTP and DNS records (SPF, DKIM, DMARC) for ixtable.com.
6. Install the biz-tools GitHub App on the ixtable org with Contents and Pull requests write access (`docs/github-app.md` in biz-tools).
7. Create the GitHub environments in biz-tools: `analytics-production`, `commerce-production`, `product-production`, `marketing-production`.
8. Decide the cookie consent approach for GA4.

### You, before public beta

9. Form the legal entity if it does not exist; apply for Apple Developer and a Windows signing certificate.
10. Reserve social handles and support mailboxes.
11. Run 10 to 15 customer interviews; confirm or change the plan prices.
12. Recruit 5 to 10 design partners.
13. Start a trademark search for "ixtable".

### You, before paid launch

14. Create the Stripe account, enable Stripe Tax, and add a restricted read key to biz-tools.
15. Commission the independent security review of credential delivery (PRD §21.3).
16. Have a lawyer review the terms, privacy policy, and DPA.
17. Make the repository public and enable the GitHub collector.

### With Claude, next

- Wire the GA4 cookie consent once you choose an approach.
- Apply the Search Console, GA4, and Supabase values you send me in `config.yaml` and the site build.
- Write the comparison pages and "Access for Mac" guide from the competitor-gap report.
- Fix `docs/concepts/deployment.md` to describe the planned Cloud distribution.
- Draft the cookie policy, subprocessors list, and `SECURITY.md`.
- Build Linux packaging and signing in CI once you have the certificates.
- Script Stripe products and prices, and build `gtm_entitlements()`.
- Ship golden apps as first-run templates.
- Draft directory listings and the Show HN post.
