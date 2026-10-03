import { createClient } from "@supabase/supabase-js";
import { test, expect } from "@playwright/test";
import { supabaseAvailable, uniqueEmail } from "./helpers";
import { SUPABASE_SERVICE_ROLE_KEY, SUPABASE_URL } from "./global-setup";

const SUPABASE_ANON_KEY =
  process.env.SUPABASE_ANON_KEY ??
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6ImFub24iLCJleHAiOjE5ODM4MTI5OTZ9.CRXP1A7WOeoJeXxjNni43kdQwgnWNReilDMblYTn_I0";

const clientOptions = { auth: { autoRefreshToken: false, persistSession: false } };

function utcDay(offset: number): string {
  const day = new Date();
  day.setUTCDate(day.getUTCDate() + offset);
  return day.toISOString().slice(0, 10);
}

test.describe("waitlist", () => {
  test.skip(
    !supabaseAvailable(),
    "Supabase is not running locally; start it with `supabase start`.",
  );

  test("joins the waitlist from the landing page", async ({ page }) => {
    const email = uniqueEmail("waitlist");
    await page.goto("/");
    const form = page.getByRole("form", { name: "Join the waitlist" }).first();
    await form.getByLabel("Email address").fill(email);
    await form.getByRole("button", { name: "Join the waitlist" }).click();
    await expect(page.getByRole("status")).toContainText("You are on the list");

    const admin = createClient(SUPABASE_URL, SUPABASE_SERVICE_ROLE_KEY, clientOptions);
    const { data, error } = await admin.from("waitlist").select("email, source").eq("email", email);
    expect(error).toBeNull();
    expect(data).toEqual([{ email, source: "website" }]);
  });

  test("an already listed email shows the same success message", async ({ page }) => {
    const email = uniqueEmail("waitlist-dupe");
    const anon = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, clientOptions);
    expect((await anon.from("waitlist").insert({ email })).error).toBeNull();

    await page.goto("/");
    const form = page.getByRole("form", { name: "Join the waitlist" }).first();
    await form.getByLabel("Email address").fill(email.toUpperCase());
    await form.getByRole("button", { name: "Join the waitlist" }).click();
    await expect(page.getByRole("status")).toContainText("You are on the list");
  });

  test("anon can insert but cannot read, update, or delete waitlist rows", async () => {
    const anon = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, clientOptions);
    const email = uniqueEmail("waitlist-rls");
    expect((await anon.from("waitlist").insert({ email })).error).toBeNull();

    const read = await anon.from("waitlist").select("email");
    expect(read.error?.code).toBe("42501");

    const update = await anon.from("waitlist").update({ source: "x" }).eq("email", email);
    expect(update.error?.code).toBe("42501");

    const remove = await anon.from("waitlist").delete().eq("email", email);
    expect(remove.error?.code).toBe("42501");

    const invalid = await anon.from("waitlist").insert({ email: "not-an-email" });
    expect(invalid.error?.code).toBe("42501");

    const forged = await anon
      .from("waitlist")
      .insert({ email: uniqueEmail("waitlist-forged"), created_at: "2020-01-01T00:00:00Z" });
    expect(forged.error?.code).toBe("42501");
  });

  test("gtm_funnel_daily is service_role only and returns zero-filled daily counts", async () => {
    const anon = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, clientOptions);
    const denied = await anon.rpc("gtm_funnel_daily", {
      start_date: utcDay(-1),
      end_date: utcDay(0),
    });
    expect(denied.error?.code).toBe("42501");

    const admin = createClient(SUPABASE_URL, SUPABASE_SERVICE_ROLE_KEY, clientOptions);
    const before = await admin.rpc("gtm_funnel_daily", {
      start_date: utcDay(0),
      end_date: utcDay(2),
    });
    expect(before.error).toBeNull();

    expect((await anon.from("waitlist").insert({ email: uniqueEmail("funnel") })).error).toBeNull();
    const { error: createError } = await admin.auth.admin.createUser({
      email: uniqueEmail("funnel-user"),
      password: "funnel-password-1",
      email_confirm: true,
    });
    expect(createError).toBeNull();

    const after = await admin.rpc("gtm_funnel_daily", {
      start_date: utcDay(0),
      end_date: utcDay(2),
    });
    expect(after.error).toBeNull();
    const rows = after.data as {
      day: string;
      waitlist_signups: number;
      account_signups: number;
      accounts_confirmed: number;
    }[];
    expect(rows.map((row) => row.day)).toEqual([utcDay(0), utcDay(1), utcDay(2)]);
    expect(Object.keys(rows[0]).sort()).toEqual(
      ["account_signups", "accounts_confirmed", "day", "waitlist_signups"].sort(),
    );
    // Nothing is created in the future, so those days must be zero-filled.
    for (const row of rows.slice(1)) {
      expect(row.waitlist_signups).toBe(0);
      expect(row.account_signups).toBe(0);
      expect(row.accounts_confirmed).toBe(0);
    }
    const today = rows[0];
    const previous = (before.data as typeof rows)[0];
    // Other specs may run in parallel, so require at least this spec's own rows.
    expect(today.waitlist_signups).toBeGreaterThanOrEqual(previous.waitlist_signups + 1);
    expect(today.account_signups).toBeGreaterThanOrEqual(previous.account_signups + 1);
    expect(today.accounts_confirmed).toBeGreaterThanOrEqual(previous.accounts_confirmed + 1);

    const tooLong = await admin.rpc("gtm_funnel_daily", {
      start_date: "2025-01-01",
      end_date: "2026-03-01",
    });
    expect(tooLong.error?.message).toMatch(/400 days/);
  });
});
