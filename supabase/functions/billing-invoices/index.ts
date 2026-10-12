// billing-invoices {appId} → {invoices: Invoice[]}
// Invoices from the billing provider for the app's organization (the billing
// customer, PRD §4.5; empty before the first checkout). Billing capability only.
import { billingProvider } from "../_shared/billing.ts";
import { appBillingCustomerId, requireBillingApp } from "../_shared/commercial.ts";
import { handler, readJson, requireUser } from "../_shared/http.ts";
import { enforceNamedRateLimit } from "../_shared/rateLimit.ts";
import { uuid } from "../_shared/validate.ts";

Deno.serve(
  handler(async (req) => {
    const { user } = await requireUser(req);
    const appId = uuid(await readJson(req), "appId");
    await enforceNamedRateLimit("billing-invoices", user.id);
    const app = await requireBillingApp(appId, user.id);
    const customerId = await appBillingCustomerId(app);
    if (!customerId) return { invoices: [] };
    const invoices = await billingProvider().listInvoices({
      customerId,
    });
    return { invoices };
  }),
);
