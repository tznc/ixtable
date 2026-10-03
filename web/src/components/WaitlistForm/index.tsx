import React, { useId, useState, type FormEvent, type ReactNode } from "react";
import clsx from "clsx";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import { getSupabaseClient } from "@site/src/lib/supabaseClient";
import styles from "./styles.module.css";

// Postgres unique_violation. The address is already listed, which the form
// reports as success so it never reveals who is on the list.
const UNIQUE_VIOLATION = "23505";

type Status = "idle" | "submitting" | "success" | "error";

interface WaitlistFormProps {
  /** Stored in `waitlist.source` to tell signup locations apart. */
  source?: string;
  id?: string;
  className?: string;
}

export default function WaitlistForm({
  source = "website",
  id,
  className,
}: WaitlistFormProps): ReactNode {
  const { siteConfig } = useDocusaurusContext();
  const { supabaseUrl, supabaseAnonKey } = siteConfig.customFields as {
    supabaseUrl: string;
    supabaseAnonKey: string;
  };
  const inputId = useId();
  const [email, setEmail] = useState("");
  const [status, setStatus] = useState<Status>("idle");

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setStatus("submitting");
    try {
      // Created on submit, never at render time: the client is browser-only.
      const supabase = getSupabaseClient(supabaseUrl, supabaseAnonKey);
      const { error } = await supabase
        .from("waitlist")
        .insert({ email: email.trim().toLowerCase(), source });
      setStatus(!error || error.code === UNIQUE_VIOLATION ? "success" : "error");
    } catch {
      setStatus("error");
    }
  };

  if (status === "success") {
    return (
      <div id={id} className={clsx(styles.form, className)}>
        <p role="status" className={styles.success} data-testid="waitlist-success">
          You are on the list. We will email you when ixtable Cloud opens.
        </p>
      </div>
    );
  }

  return (
    <form
      id={id}
      aria-label="Join the waitlist"
      className={clsx(styles.form, className)}
      onSubmit={handleSubmit}
    >
      <div className={styles.row}>
        <label htmlFor={inputId} className={styles.label}>
          Email address
        </label>
        <input
          id={inputId}
          type="email"
          name="email"
          autoComplete="email"
          required
          maxLength={254}
          placeholder="you@company.com"
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          aria-describedby={status === "error" ? `${inputId}-error` : undefined}
          className={styles.input}
          data-testid="waitlist-email-input"
        />
        <button
          type="submit"
          className={styles.button}
          disabled={status === "submitting"}
          data-testid="waitlist-submit"
        >
          Join the waitlist
        </button>
      </div>
      {status === "error" && (
        <p id={`${inputId}-error`} role="alert" className={styles.error}>
          Something went wrong. Check the address and try again.
        </p>
      )}
    </form>
  );
}
