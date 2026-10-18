import React, { useState, type ReactNode } from "react";
import {
  formatDate,
  formatPrice,
  useCloudApi,
  type FunctionName,
  type Plan,
} from "@site/src/lib/cloud";
import { entitlementStatus } from "@site/src/lib/cloud/status";
import {
  Badge,
  ConfirmDialog,
  Empty,
  ErrorNotice,
  Loading,
  Notice,
  Section,
  TableWrap,
} from "../ui";
import { useAction, useAsync } from "../useAsync";
import type { AppTabProps } from "./types";

function money(cents: number, currency: string): string {
  return (cents / 100).toLocaleString(undefined, {
    style: "currency",
    currency: currency.toUpperCase(),
  });
}

function PlanCard({
  plan,
  current,
  used,
  onChoose,
  pending,
}: {
  plan: Plan;
  current: boolean;
  used: number;
  onChoose: (plan: Plan) => void;
  pending: boolean;
}): ReactNode {
  const tooSmall = used > plan.runtime_user_allowance;
  return (
    <div className="pricing-card">
      <h3>
        {plan.name} {current && <Badge tone="success">Current plan</Badge>}
      </h3>
      <div className="pricing-price">{formatPrice(plan.price_cents, plan.interval)}</div>
      <ul>
        <li>{plan.runtime_user_allowance} runtime users</li>
        <li>{plan.storage_gb} GB archive storage</li>
      </ul>
      {tooSmall && (
        <p className="cloud-muted">
          You have {used} active runtime users. Revoke some before choosing this plan.
        </p>
      )}
      {!current && (
        <button
          type="button"
          className="button button--primary"
          disabled={pending || tooSmall}
          onClick={() => onChoose(plan)}
        >
          Choose {plan.name}
        </button>
      )}
    </div>
  );
}

/** Plan, allowance, checkout, portal, invoices, and cancellation for one app. */
export default function BillingTab({ app, entitlement, reloadApp }: AppTabProps): ReactNode {
  const api = useCloudApi();
  const [confirmCancel, setConfirmCancel] = useState(false);
  const state = useAsync(async () => {
    const q = api.q();
    const [plans, subscriptions] = await Promise.all([q.plans(), q.subscriptions([app.id])]);
    return { plans, subscription: subscriptions[app.id] ?? null };
  }, [api, app.id]);
  const invoices = useAsync(
    async () => (await api.call("billing-invoices", { appId: app.id })).invoices,
    [api, app.id],
  );
  const redirect = useAction(async (name: FunctionName, planId?: string) => {
    const result =
      name === "billing-checkout"
        ? await api.call("billing-checkout", { appId: app.id, planId: planId ?? "" })
        : await api.call("billing-portal", { appId: app.id });
    window.location.assign((result as { url: string }).url);
  });
  const cancel = useAction(async () => {
    await api.call("billing-cancel", { appId: app.id, atPeriodEnd: true });
    setConfirmCancel(false);
    state.reload();
    reloadApp();
  });

  if (state.loading && !state.data) return <Loading label="Loading billing" />;
  const subscription = state.data?.subscription ?? null;
  const plans = state.data?.plans ?? [];
  const status = entitlementStatus(entitlement);
  const used = entitlement?.used ?? 0;
  return (
    <>
      <ErrorNotice error={state.error ?? redirect.error} testId="billing-error" />
      <Section
        title="Current plan"
        description="ixtable Cloud is priced per app. Each plan includes a runtime user allowance."
      >
        <dl className="cloud-dl">
          <dt>Plan</dt>
          <dd data-testid="billing-plan">
            {plans.find((plan) => plan.id === subscription?.plan_id)?.name ?? "No plan"}
          </dd>
          <dt>Status</dt>
          <dd>
            <Badge tone={status.tone}>{status.label}</Badge> {status.help}
          </dd>
          <dt>Runtime users</dt>
          <dd>{entitlement ? `${entitlement.used} of ${entitlement.allowance}` : "Unknown"}</dd>
          {subscription && (
            <>
              <dt>{subscription.cancel_at_period_end ? "Ends" : "Renews"}</dt>
              <dd>{formatDate(subscription.current_period_end)}</dd>
            </>
          )}
        </dl>
        {subscription?.cancel_at_period_end && (
          <Notice tone="warning">
            The subscription ends on {formatDate(subscription.current_period_end)}. After that,
            runtime users cannot sync or get credential keys until you choose a plan again.
          </Notice>
        )}
        {subscription && (
          <div className="cloud-actions">
            <button
              type="button"
              className="button button--secondary"
              disabled={redirect.pending}
              onClick={() => redirect.run("billing-portal")}
            >
              Manage payment method
            </button>
            {!subscription.cancel_at_period_end && subscription.status !== "canceled" && (
              <button
                type="button"
                className="button button--outline button--danger"
                onClick={() => setConfirmCancel(true)}
              >
                Cancel subscription
              </button>
            )}
          </div>
        )}
      </Section>
      <Section title={subscription ? "Change plan" : "Choose a plan"}>
        <div className="pricing-grid">
          {plans.map((plan) => (
            <PlanCard
              key={plan.id}
              plan={plan}
              current={plan.id === subscription?.plan_id && subscription.status !== "canceled"}
              used={used}
              pending={redirect.pending}
              onChoose={(row) => redirect.run("billing-checkout", row.id)}
            />
          ))}
        </div>
      </Section>
      <Section title="Invoices">
        {invoices.loading && !invoices.data ? (
          <Loading label="Loading invoices" />
        ) : invoices.error ? (
          <p className="cloud-muted">
            Invoices are not available here right now. Open Manage payment method to see them.
          </p>
        ) : !invoices.data?.length ? (
          <Empty>No invoices yet.</Empty>
        ) : (
          <TableWrap label="Invoices">
            <thead>
              <tr>
                <th scope="col">Invoice</th>
                <th scope="col">Date</th>
                <th scope="col">Amount</th>
                <th scope="col">Status</th>
              </tr>
            </thead>
            <tbody>
              {invoices.data.map((invoice) => (
                <tr key={invoice.id}>
                  <th scope="row">
                    {invoice.hostedInvoiceUrl ? (
                      <a href={invoice.hostedInvoiceUrl}>{invoice.number ?? invoice.id}</a>
                    ) : (
                      (invoice.number ?? invoice.id)
                    )}
                  </th>
                  <td>{formatDate(invoice.created)}</td>
                  <td>{money(invoice.amountDue, invoice.currency)}</td>
                  <td>{invoice.status ?? "Unknown"}</td>
                </tr>
              ))}
            </tbody>
          </TableWrap>
        )}
      </Section>
      <ConfirmDialog
        open={confirmCancel}
        title="Cancel subscription?"
        confirmLabel="Cancel at period end"
        danger
        pending={cancel.pending}
        error={cancel.error}
        onConfirm={() => cancel.run()}
        onCancel={() => setConfirmCancel(false)}
      >
        <p>
          The plan stays active until {formatDate(subscription?.current_period_end)}. After that,
          runtime users of {app.name} cannot install, sync, or get credential keys.
        </p>
      </ConfirmDialog>
    </>
  );
}
