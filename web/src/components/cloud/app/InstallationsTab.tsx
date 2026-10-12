import React, { useState, type ReactNode } from "react";
import { formatDate, shortId, useCloudApi, type Installation } from "@site/src/lib/cloud";
import { Badge, ConfirmDialog, Empty, ErrorNotice, Loading, TableWrap } from "../ui";
import { useAction, useAsync } from "../useAsync";
import type { AppTabProps } from "./types";

/** Runtime installations (devices) with last sync, installed version, and device revocation. */
export default function InstallationsTab({ app, isAdmin }: AppTabProps): ReactNode {
  const api = useCloudApi();
  const [target, setTarget] = useState<Installation | null>(null);
  const state = useAsync(async () => {
    const q = api.q();
    const [installations, versions] = await Promise.all([
      q.installations(app.id),
      q.versions(app.id),
    ]);
    const profiles = await q.profiles(installations.map((row) => row.user_id));
    return { installations, versions, profiles };
  }, [api, app.id]);
  const revoke = useAction(async () => {
    if (!target) return;
    await api.call("devices-revoke", {
      appId: app.id,
      userId: target.user_id,
      installationId: target.id,
    });
    setTarget(null);
    state.reload();
  });

  if (state.loading && !state.data) return <Loading label="Loading installations" />;
  const data = state.data;
  const userOf = (row: Installation) => data?.profiles[row.user_id]?.email || shortId(row.user_id);
  return (
    <>
      <ErrorNotice error={state.error} />
      {data?.installations.length === 0 && <Empty>No runtime installations yet.</Empty>}
      {data && data.installations.length > 0 && (
        <TableWrap label="Installations">
          <thead>
            <tr>
              <th scope="col">Device</th>
              <th scope="col">User</th>
              <th scope="col">Version</th>
              <th scope="col">Last sync</th>
              <th scope="col">Status</th>
              {isAdmin && <th scope="col">Action</th>}
            </tr>
          </thead>
          <tbody>
            {data.installations.map((row) => {
              const version = data.versions.find((v) => v.id === row.installed_version_id);
              const device = row.device_name || shortId(row.id);
              return (
                <tr key={row.id}>
                  <th scope="row">{device}</th>
                  <td>{userOf(row)}</td>
                  <td>{version?.version ?? "Unknown"}</td>
                  <td>{formatDate(row.last_seen_at)}</td>
                  <td>
                    {row.revoked_at ? (
                      <Badge tone="danger">Revoked {formatDate(row.revoked_at)}</Badge>
                    ) : (
                      <Badge tone="success">Active</Badge>
                    )}
                  </td>
                  {isAdmin && (
                    <td>
                      {!row.revoked_at && (
                        <button
                          type="button"
                          className="button button--sm button--outline button--danger"
                          onClick={() => setTarget(row)}
                        >
                          Revoke {device}
                        </button>
                      )}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </TableWrap>
      )}
      <ConfirmDialog
        open={target !== null}
        title="Revoke this device?"
        confirmLabel="Revoke device"
        danger
        pending={revoke.pending}
        error={revoke.error}
        onConfirm={() => revoke.run()}
        onCancel={() => setTarget(null)}
      >
        <p>
          {target && `${target.device_name || shortId(target.id)} (${userOf(target)})`} can no
          longer sync, download bundles, or get credential keys. A key grant it already holds stays
          valid until it expires, at most 24 hours. Data already on the device is not erased. The
          user keeps access on their other devices.
        </p>
      </ConfirmDialog>
    </>
  );
}
