// Settings → Organization management (ADR 0022, better-auth pattern).
//
// Members table (role dropdown, remove, invite), role editor (resource×action
// permission matrix; presets read-only), and pending invitations. The server is
// authoritative; the UI surfaces server errors inline. Composes the .ol-*
// primitive library; only layout-only .org-* classes live in index.css.

import { useCallback, useEffect, useMemo, useState } from "react";
import { Icon } from "../components/Icon";
import {
  fetchMembers,
  fetchRoles,
  fetchInvitations,
  inviteMember,
  setMemberRole,
  removeMember,
  saveRole,
  deleteRole,
  revokeInvitation,
  createUser,
  type OrgMember,
  type OrgRole,
  type StatementEntry,
  type OrgInvitation,
} from "../api";

export function SettingsView() {
  return (
    <div className="shell-placeholder">
      <div className="gv-head">
        <Icon name="gear" size={14} />
        <span className="gv-title">Settings</span>
        <span className="gtag">ORGANIZATION</span>
      </div>
      <div className="org-body">
        <OrganizationSettings />
      </div>
    </div>
  );
}

function OrganizationSettings() {
  const [members, setMembers] = useState<OrgMember[]>([]);
  const [roles, setRoles] = useState<OrgRole[]>([]);
  const [statement, setStatement] = useState<StatementEntry[]>([]);
  const [invites, setInvites] = useState<OrgInvitation[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [editingRole, setEditingRole] = useState<OrgRole | "new" | null>(null);
  const [inviteToken, setInviteToken] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setErr(null);
    try {
      const [m, r, i] = await Promise.all([fetchMembers(), fetchRoles(), fetchInvitations()]);
      setMembers(m);
      setRoles(r.roles);
      setStatement(r.statement);
      setInvites(i);
    } catch (e) {
      setErr(String((e as Error).message ?? e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const roleNames = useMemo(() => roles.map((r) => r.name), [roles]);
  const onErr = useCallback((e: unknown) => setErr(String((e as Error).message ?? e)), []);

  if (loading) return <div className="org-loading">Loading organization…</div>;

  return (
    <div className="org-settings">
      {err && (
        <div className="ol-badge ol-badge-err org-error" role="alert">
          <Icon name="alert" size={12} /> {err}
        </div>
      )}

      <section className="org-section">
        <header className="org-sec-head">
          <h3 className="ol-field-label">Members</h3>
          <InviteControl
            roleNames={roleNames}
            onInvited={(t) => {
              setInviteToken(t);
              void reload();
            }}
          />
        </header>
        {inviteToken && (
          <div className="org-invite-token">
            <span>Invitation link (share once):</span>
            <code>/api/auth/invitations/{inviteToken}/accept</code>
            <button type="button" className="ol-btn ol-btn-sm ol-btn-ghost" onClick={() => setInviteToken(null)}>
              dismiss
            </button>
          </div>
        )}
        <table className="org-table">
          <thead>
            <tr>
              <th>User</th>
              <th>Role</th>
              <th aria-label="actions" />
            </tr>
          </thead>
          <tbody>
            {members.map((m) => (
              <tr key={m.userId}>
                <td>{m.username}</td>
                <td>
                  <select
                    className="ol-select org-role-select"
                    value={m.role}
                    onChange={(e) =>
                      setMemberRole(m.userId, e.target.value).then(reload).catch(onErr)
                    }
                  >
                    {roleNames.map((n) => (
                      <option key={n} value={n}>{n}</option>
                    ))}
                    {!roleNames.includes(m.role) && <option value={m.role}>{m.role}</option>}
                  </select>
                </td>
                <td className="org-cell-actions">
                  <button
                    type="button"
                    className="ol-iconbtn"
                    title="Remove from organization"
                    onClick={() => removeMember(m.userId).then(reload).catch(onErr)}
                  >
                    <Icon name="trash" size={12} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="org-section">
        <header className="org-sec-head">
          <h3 className="ol-field-label">Roles</h3>
          <button type="button" className="ol-btn ol-btn-sm" onClick={() => setEditingRole("new")}>
            <Icon name="plus" size={12} /> New role
          </button>
        </header>
        <div className="org-roles">
          {roles.map((r) => (
            <div key={r.name} className="ol-card org-role-card">
              <div className="org-role-name">
                {r.name}
                {r.builtin && <span className="ol-badge">preset</span>}
              </div>
              <PermissionSummary permissions={r.permissions} />
              <div className="org-role-actions">
                <button type="button" className="ol-btn ol-btn-sm" onClick={() => setEditingRole(r)}>
                  {r.builtin ? "View" : "Edit"}
                </button>
                {!r.builtin && (
                  <button
                    type="button"
                    className="ol-btn ol-btn-sm ol-btn-danger"
                    onClick={() => deleteRole(r.name).then(reload).catch(onErr)}
                  >
                    Delete
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      </section>

      {invites.length > 0 && (
        <section className="org-section">
          <header className="org-sec-head">
            <h3 className="ol-field-label">Pending invitations</h3>
          </header>
          <table className="org-table">
            <thead>
              <tr>
                <th>Invitee</th>
                <th>Role</th>
                <th>Status</th>
                <th aria-label="actions" />
              </tr>
            </thead>
            <tbody>
              {invites.map((i) => (
                <tr key={i.id}>
                  <td>{i.emailOrUsername}</td>
                  <td>{i.roleName}</td>
                  <td>
                    <span className={`ol-badge ${i.status === "pending" ? "ol-badge-warn" : ""}`}>
                      {i.status}
                    </span>
                  </td>
                  <td className="org-cell-actions">
                    {i.status === "pending" && (
                      <button
                        type="button"
                        className="ol-btn ol-btn-sm ol-btn-danger"
                        onClick={() => revokeInvitation(i.id).then(reload).catch(onErr)}
                      >
                        revoke
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {editingRole && (
        <RoleEditor
          role={editingRole === "new" ? null : editingRole}
          statement={statement}
          onClose={() => setEditingRole(null)}
          onSaved={() => {
            setEditingRole(null);
            void reload();
          }}
          onError={(e) => setErr(e)}
        />
      )}
    </div>
  );
}

function PermissionSummary({ permissions }: { permissions: string }) {
  let parsed: Record<string, string[]> = {};
  try {
    parsed = JSON.parse(permissions);
  } catch {
    /* malformed statement — show nothing rather than crash */
  }
  if (parsed["*"]) return <div className="org-perm-summary">all permissions</div>;
  const count = Object.values(parsed).reduce((n, a) => n + a.length, 0);
  return (
    <div className="org-perm-summary">
      {count} permission{count === 1 ? "" : "s"} · {Object.keys(parsed).length} resource
      {Object.keys(parsed).length === 1 ? "" : "s"}
    </div>
  );
}

function InviteControl({
  roleNames,
  onInvited,
}: {
  roleNames: string[];
  onInvited: (token: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [username, setUsername] = useState("");
  const [role, setRole] = useState(roleNames.includes("member") ? "member" : roleNames[0] ?? "");
  const [makeUser, setMakeUser] = useState(false);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [localErr, setLocalErr] = useState<string | null>(null);

  if (!open) {
    return (
      <button type="button" className="ol-btn ol-btn-sm ol-btn-primary" onClick={() => setOpen(true)}>
        <Icon name="plus" size={12} /> Invite
      </button>
    );
  }
  return (
    <div className="org-invite-form">
      <input
        className="ol-input"
        placeholder="username"
        value={username}
        onChange={(e) => setUsername(e.target.value)}
      />
      <select className="ol-select" value={role} onChange={(e) => setRole(e.target.value)}>
        {roleNames.map((n) => (
          <option key={n} value={n}>{n}</option>
        ))}
      </select>
      <label className="ol-check">
        <input type="checkbox" checked={makeUser} onChange={(e) => setMakeUser(e.target.checked)} />
        create user
      </label>
      {makeUser && (
        <input
          className="ol-input"
          type="password"
          placeholder="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      )}
      {localErr && <span className="org-inline-err">{localErr}</span>}
      <button
        type="button"
        className="ol-btn ol-btn-sm ol-btn-primary"
        disabled={busy || !username || !role || (makeUser && !password)}
        onClick={async () => {
          setBusy(true);
          setLocalErr(null);
          try {
            if (makeUser) await createUser(username, password);
            const { token } = await inviteMember(username, role);
            onInvited(token);
            setOpen(false);
            setUsername("");
            setPassword("");
            setMakeUser(false);
          } catch (e) {
            setLocalErr(String((e as Error).message));
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy ? "…" : "Send"}
      </button>
      <button type="button" className="ol-btn ol-btn-sm ol-btn-ghost" onClick={() => setOpen(false)}>
        cancel
      </button>
    </div>
  );
}

function RoleEditor({
  role,
  statement,
  onClose,
  onSaved,
  onError,
}: {
  role: OrgRole | null;
  statement: StatementEntry[];
  onClose: () => void;
  onSaved: () => void;
  onError: (e: string) => void;
}) {
  const readOnly = role?.builtin ?? false;
  const initial = useMemo<Record<string, Set<string>>>(() => {
    const out: Record<string, Set<string>> = {};
    if (role) {
      try {
        const parsed: Record<string, string[]> = JSON.parse(role.permissions);
        for (const [res, acts] of Object.entries(parsed)) out[res] = new Set(acts);
      } catch {
        /* malformed statement — start empty */
      }
    }
    return out;
  }, [role]);
  const [grants, setGrants] = useState(initial);
  const [name, setName] = useState(role?.name ?? "");
  const [busy, setBusy] = useState(false);
  const wildcard = !!grants["*"];

  const toggle = (res: string, act: string) => {
    if (readOnly) return;
    setGrants((prev) => {
      const next = { ...prev };
      const set = new Set(next[res] ?? []);
      if (set.has(act)) set.delete(act);
      else set.add(act);
      if (set.size === 0) delete next[res];
      else next[res] = set;
      return next;
    });
  };

  return (
    <div className="ol-overlay" onClick={onClose}>
      <div className="ol-dialog org-role-dialog" onClick={(e) => e.stopPropagation()}>
        <header className="ol-dialog-head">
          <div className="ol-dialog-title">
            {role ? (readOnly ? `Role: ${role.name} (preset)` : `Edit role: ${role.name}`) : "New role"}
          </div>
          <button type="button" className="ol-iconbtn" onClick={onClose} aria-label="Close">
            <Icon name="x" size={14} />
          </button>
        </header>
        <div className="ol-dialog-body">
          {!role && (
            <input
              className="ol-input org-role-name-input"
              placeholder="role name (lowercase, dashes)"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          )}
          {wildcard ? (
            <div className="org-perm-summary">This role grants all permissions (owner).</div>
          ) : (
            <div className="org-matrix">
              {statement.map((entry) => (
                <div key={entry.resource} className="org-matrix-row">
                  <div className="org-matrix-res">{entry.resource}</div>
                  <div className="org-matrix-acts">
                    {entry.actions.map((act) => {
                      const on = grants[entry.resource]?.has(act) ?? false;
                      return (
                        <label
                          key={act}
                          className={`ol-tag ol-tag-btn ${on ? "ol-tag-active" : ""}`}
                        >
                          <input
                            type="checkbox"
                            className="org-chip-input"
                            checked={on}
                            disabled={readOnly}
                            onChange={() => toggle(entry.resource, act)}
                          />
                          {act}
                        </label>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
        {!readOnly && (
          <footer className="ol-dialog-foot">
            <button type="button" className="ol-btn ol-btn-ghost" onClick={onClose}>
              cancel
            </button>
            <button
              type="button"
              className="ol-btn ol-btn-primary"
              disabled={busy || (!role && !name)}
              onClick={async () => {
                setBusy(true);
                try {
                  const perms: Record<string, string[]> = {};
                  for (const [res, set] of Object.entries(grants)) perms[res] = [...set];
                  await saveRole(role?.name ?? name, perms, !role);
                  onSaved();
                } catch (e) {
                  onError(String((e as Error).message));
                } finally {
                  setBusy(false);
                }
              }}
            >
              {busy ? "Saving…" : "Save role"}
            </button>
          </footer>
        )}
      </div>
    </div>
  );
}
