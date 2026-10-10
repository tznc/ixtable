import { useMemo } from "react";
import type { SupabaseClient } from "@supabase/supabase-js";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import { getSupabaseClient } from "@site/src/lib/supabaseClient";
import { invokeFunction } from "./functions";
import { createQueries, type CloudQueries } from "./queries";
import type { FunctionMap, FunctionName } from "./types";

export { CloudError, friendlyMessage, toCloudError } from "./errors";
export type * from "./types";

export interface SiteFlags {
  supabaseUrl: string;
  supabaseAnonKey: string;
  oauthEnabled: boolean;
  googleAuthEnabled: boolean;
  microsoftAuthEnabled: boolean;
}

export function useSiteFlags(): SiteFlags {
  const { siteConfig } = useDocusaurusContext();
  return siteConfig.customFields as unknown as SiteFlags;
}

export interface CloudApi {
  /** The browser Supabase client. Call only from effects and event handlers. */
  client(): SupabaseClient;
  /** PostgREST reads and owner-scoped writes under RLS. */
  q(): CloudQueries;
  /** Typed Edge Function call. */
  call<K extends FunctionName>(
    name: K,
    input: FunctionMap[K]["in"],
  ): Promise<FunctionMap[K]["out"]>;
}

/** Returns a stable adapter. Nothing touches Supabase until a method runs in the browser. */
export function useCloudApi(): CloudApi {
  const { supabaseUrl, supabaseAnonKey } = useSiteFlags();
  return useMemo(() => {
    const client = () => getSupabaseClient(supabaseUrl, supabaseAnonKey);
    return {
      client,
      q: () => createQueries(client()),
      call: (name, input) => invokeFunction(client(), name, input),
    };
  }, [supabaseUrl, supabaseAnonKey]);
}

/** Accepts only same-site absolute paths so `?next=` cannot redirect off-site. */
export function safeNext(value: string | null | undefined, fallback = "/account"): string {
  if (!value || !value.startsWith("/") || value.startsWith("//") || value.includes("\\")) {
    return fallback;
  }
  return value;
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || Number.isNaN(bytes)) return "Unknown";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

export function formatDate(value: string | null | undefined): string {
  if (!value) return "Never";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function formatPrice(cents: number, interval: string): string {
  const amount = (cents / 100).toLocaleString(undefined, { style: "currency", currency: "USD" });
  return `${amount} per ${interval}`;
}

/** Price line for a plan at an interval: the annual price is billed once per year. */
export function planPrice(
  plan: { price_cents: number; annual_price_cents: number },
  interval: "month" | "year",
): string {
  return interval === "year"
    ? formatPrice(plan.annual_price_cents, "year")
    : formatPrice(plan.price_cents, "month");
}

export const ARCHIVE_LIMIT_BYTES = 500 * 1024 * 1024;

export function shortId(id: string | null | undefined): string {
  return id ? id.slice(0, 8) : "None";
}
