/**
 * Health gate: fail fast, with the fix, when the local stack is not up.
 * Checks Auth, PostgREST and the Edge runtime (functions/v1/health).
 */
import { functionsUrl, localStack } from "./clients";

async function probe(url: string, headers: Record<string, string> = {}): Promise<string | null> {
  try {
    const response = await fetch(url, { headers, signal: AbortSignal.timeout(30_000) });
    if (!response.ok) return `HTTP ${response.status} ${(await response.text()).slice(0, 200)}`;
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

export async function assertLocalStackUp(): Promise<void> {
  const { url, anonKey } = localStack();
  const failures: string[] = [];
  const auth = await probe(`${url}/auth/v1/health`, { apikey: anonKey });
  if (auth) failures.push(`auth: ${auth}`);
  const rest = await probe(`${url}/rest/v1/`, { apikey: anonKey });
  if (rest) failures.push(`rest: ${rest}`);
  const edge = await probe(functionsUrl("health"));
  if (edge) failures.push(`functions/v1/health: ${edge}`);
  if (failures.length > 0) {
    throw new Error(
      `Local Supabase stack at ${url} is not healthy (${failures.join("; ")}). ` +
        "Run `npm run service-qa:up --prefix web` from the repo root (see .claude/skills/service-qa/SKILL.md).",
    );
  }
}
