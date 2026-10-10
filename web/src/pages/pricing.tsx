import React, { useEffect, useState, type ReactNode } from "react";
import Layout from "@theme/Layout";
import Link from "@docusaurus/Link";
import { useAuth } from "@site/src/contexts/AuthContext";
import { planPrice, useCloudApi, type BillingInterval, type Plan } from "@site/src/lib/cloud";
import IntervalToggle from "@site/src/components/cloud/IntervalToggle";
import catalog from "@site/src/data/plans.json";

const STATIC_PLANS = catalog.plans as Plan[];

const INCLUDED = [
  "Private distribution to invited runtime users",
  "Signed, user-fingerprinted runtime bundles",
  "Encrypted datasource credential delivery",
  "Automatic updates on sync",
  "Archive versions, installation backups, and restore",
  "Audit history",
];

/**
 * Plans are per cloud app. Signed-in visitors see the live catalog from `public.plans`; RLS hides
 * it from anonymous visitors, who see the static copy in src/data/plans.json.
 */
export default function PricingPage(): ReactNode {
  const { user, loading } = useAuth();
  const api = useCloudApi();
  const [plans, setPlans] = useState<Plan[]>(STATIC_PLANS);
  const [interval, setInterval] = useState<BillingInterval>("month");
  const [live, setLive] = useState(false);

  useEffect(() => {
    if (loading || !user) return;
    let cancelled = false;
    api
      .q()
      .plans()
      .then((rows) => {
        if (!cancelled && rows.length > 0) {
          setPlans(rows);
          setLive(true);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [api, loading, user]);

  return (
    <Layout title="Pricing" description="ixtable Cloud plans, priced per app">
      <main className="cloud-page">
        <h1>Pricing</h1>
        <p>
          The ixtable desktop app is free and open source. You pay only for ixtable Cloud, per cloud
          app. Each plan includes a number of runtime users: the people you invite to run the app.
          You, the app's developer, are not counted.
        </p>
        <p data-testid="pricing-terms">
          Billed per app. Annual billing: 2 months free. No free trial.
        </p>
        <IntervalToggle name="pricing-interval" value={interval} onChange={setInterval} />
        <div className="pricing-grid">
          {plans.map((plan) => (
            <section key={plan.id} className="pricing-card" aria-label={`${plan.name} plan`}>
              <h2>{plan.name}</h2>
              <div className="pricing-price">{planPrice(plan, interval)}</div>
              {interval === "year" && <p className="cloud-muted">2 months free</p>}
              <ul>
                <li>{plan.runtime_user_allowance} runtime users</li>
                <li>{plan.storage_gb} GB archive storage</li>
                <li>500 MB per archive</li>
              </ul>
              <Link
                className="button button--primary"
                to={user ? "/cloud" : "/signup?next=%2Fcloud"}
              >
                {user ? "Open the Cloud dashboard" : "Create an account"}
              </Link>
            </section>
          ))}
        </div>
        <p className="cloud-muted" data-testid="pricing-source">
          {live ? "Current prices from ixtable Cloud." : "Prices in US dollars, before tax."}
        </p>
        <h2>Every plan includes</h2>
        <ul>
          {INCLUDED.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
        <h2>What ixtable Cloud is not</h2>
        <p>
          ixtable Cloud does not host your PostgreSQL database, run apps in the browser, or sync
          SQLite records between users. Each runtime installation keeps its own local records. Read
          the <Link to="/docs/cloud/security">security model</Link> before you distribute an app
          that connects to PostgreSQL.
        </p>
      </main>
    </Layout>
  );
}
