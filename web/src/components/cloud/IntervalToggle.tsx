import React, { type ReactNode } from "react";
import type { BillingInterval } from "@site/src/lib/cloud";

/** Monthly / Annual choice as a labelled radio group styled as two buttons. */
export default function IntervalToggle({
  value,
  onChange,
  name,
}: {
  value: BillingInterval;
  onChange: (interval: BillingInterval) => void;
  name: string;
}): ReactNode {
  return (
    <fieldset className="cloud-actions" style={{ border: 0, padding: 0, margin: "0 0 1rem" }}>
      <legend className="cloud-muted">Billing interval</legend>
      {(
        [
          ["month", "Monthly"],
          ["year", "Annual (2 months free)"],
        ] as const
      ).map(([interval, label]) => (
        <label
          key={interval}
          className={`button button--sm ${value === interval ? "button--primary" : "button--secondary"}`}
        >
          <input
            type="radio"
            name={name}
            value={interval}
            checked={value === interval}
            onChange={() => onChange(interval)}
            style={{ position: "absolute", opacity: 0, width: 1, height: 1 }}
          />
          {label}
        </label>
      ))}
    </fieldset>
  );
}
