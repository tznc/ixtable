// billing-portal {appId} → {url}
// Provider billing portal (payment method, invoices, plan changes) for an
// app's organization (the billing customer, PRD §4.5). Billing capability only.
import { billingProvider } from "../_shared/billing.ts";
import { appBillingCustomerId, appBillingUrl, requireBillingApp } from "../_shared/commercial.ts";
import { handler, HttpError, readJson, requireUser } from "../_shared/http.ts";
import { enforceNamedRateLimit } from "../_shared/rateLimit.ts";
import { uuid } from "../_shared/validate.ts";

Deno.serve(
  handler(async (req) => {
    const { user } = await requireUser(req);
    const appId = uuid(await readJson(req), "appId");
    await enforceNamedRateLimit("billing-portal", user.id);
    const app = await requireBillingApp(appId, user.id);
    const customerId = await appBillingCustomerId(app);
    if (!customerId) {
      throw new HttpError("VALIDATION", "appId: the app has no billing account yet", {
        field: "appId",
      });
    }
    const { url } = await billingProvider().createPortal({
      customerId,
      appId,
      returnUrl: appBillingUrl(appId),
    });
    return { url };
  }),
);
