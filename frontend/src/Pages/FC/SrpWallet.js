import React from "react";
import styled from "styled-components";
import { AuthContext, ToastContext } from "../../contexts";
import { apiCall, errorToaster, toaster } from "../../api";
import { Button, Buttons, Select } from "../../Components/Form";
import { Confirm } from "../../Components/Modal";
import { InfoNote } from "../../Components/NoteBox";
import { Content, PageTitle } from "../../Components/Page";
import { Cell, CellHead, Row, Table, TableBody, TableHead } from "../../Components/Table";
import { addToast } from "../../Components/Toast";
import { formatDatetime } from "../../Util/time";
import { formatAgo, formatClockUtc, formatIsk, formatSpan } from "../../Util/srp";

const POLL_MS = 60000;
// Right after connecting, the first check is due within a minute, so look more often until it lands
const CHECKING_POLL_MS = 5000;
const CHECKING_WINDOW_SECONDS = 10 * 60;
// The server checks the wallet every few minutes, so this long without a success means it is
// probably not running the check at all
const STALE_SECONDS = 15 * 60;

const Muted = styled.p`
  color: ${(props) => props.theme.colors.accent4};
`;

const Hint = styled.span`
  color: ${(props) => props.theme.colors.accent4};
  font-size: 0.9em;
`;

const TableScroll = styled.div`
  overflow-x: auto;
  max-width: 900px;
`;

const Details = styled.dl`
  display: grid;
  grid-template-columns: max-content minmax(0, 1fr);
  gap: 0.2em 1.2em;
  margin: 0.7em 0 1em;
  line-height: 1.5em;

  dt {
    color: ${(props) => props.theme.colors.accent4};
  }
  dd {
    margin: 0;
    word-break: break-word;
  }
`;

const Step = styled.section`
  margin: 1.2em 0;
`;

const StepHeading = styled.h3`
  margin: 0 0 0.3em;
`;

const StepNumber = styled.span`
  display: inline-block;
  min-width: 1.6em;
  margin-right: 0.5em;
  border-radius: 1em;
  background-color: ${(props) => props.theme.colors.secondary.color};
  color: ${(props) => props.theme.colors.secondary.text};
  font-size: 0.85em;
  line-height: 1.6em;
  text-align: center;
`;

// Groups the selects for assistive tech without a visible frame or caption: the step heading above
// is already the caption
const Fieldset = styled.fieldset`
  min-width: 0;
  margin: 0;
  padding: 0;
  border: 0;
`;

const HiddenLegend = styled.legend`
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
`;

const WalletBlock = styled.div`
  margin: 0.8em 0;
  padding: 0.7em 0.9em;
  border: 1px solid ${(props) => props.theme.colors.accent2};
  border-radius: 4px;
`;

const WalletHead = styled.div`
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 0 1em;
  margin-bottom: 0.5em;
  word-break: break-word;
`;

const Toggle = styled.summary`
  cursor: pointer;
  padding: 0.3em 0;
`;

const WaitlistRow = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.3em 1em;
  padding: 0.5em 0;
  border-bottom: 1px solid ${(props) => props.theme.colors.accent2};
  word-break: break-word;

  > .name {
    flex: 1 1 12em;
    min-width: 0;
  }
  > .pick {
    flex: 0 1 18em;
    min-width: 0;
  }
  > .pick select {
    width: 100%;
  }
  > .note {
    flex: 1 1 100%;
  }
`;

// Not InfoNote: its fixed "!" icon is wrong on the healthy state, and its white text fails contrast
// on the Light theme's green. The colour is only the edge; the glyph and words carry the state.
const Status = styled.div`
  width: fit-content;
  max-width: 100%;
  margin-bottom: 0.5em;
  padding: 0.4em 0.8em;
  border: 1px solid ${(props) => props.theme.colors.accent2};
  border-left: 4px solid ${(props) => props.theme.colors[props.variant].color};
  border-radius: 4px;
  background-color: ${(props) => props.theme.colors.background};
  color: ${(props) => props.theme.colors.text};
`;

// The warning sign in its plain text form: without U+FE0E some systems draw a coloured emoji
const WARN = "⚠︎";

const datetime = (unixSeconds) => formatDatetime(new Date(unixSeconds * 1000));

async function startConnect(characterId) {
  const url = await apiCall(`/api/srp/admin/connect?character_id=${characterId}`, {});
  window.location.href = url;
}

async function disconnect(wallet) {
  await apiCall("/api/srp/admin/disconnect", { json: { corporation_id: wallet.corporation_id } });
  return `${wallet.corporation_name} wallet disconnected`;
}

// Just connected: no check or error reported yet, and the connection is new enough that the first
// check is still expected
function isChecking(admin, wallet, now) {
  return (
    admin.enabled &&
    !wallet.checked_at &&
    !wallet.last_error &&
    now / 1000 - wallet.connected_at < CHECKING_WINDOW_SECONDS
  );
}

// Loads the admin status and keeps it fresh while the tab is visible, so the status lines don't go
// stale while the page stays open. A failed refresh keeps what is on screen (only a failed first
// load leaves the page empty); `now` still moves on, so the "last check" age keeps growing and the
// stale warning can appear. A response is dropped if a newer request has started since, or if the
// page is gone.
function useSrpAdmin() {
  const toastContext = React.useContext(ToastContext);
  const [admin, setAdmin] = React.useState(null);
  const [now, setNow] = React.useState(() => Date.now());
  const mounted = React.useRef(false);
  const latest = React.useRef(0);

  const load = React.useCallback(
    (quiet) => {
      const id = ++latest.current;
      const request = apiCall("/api/srp/admin", {}).then(
        (data) => {
          if (mounted.current && id === latest.current) setAdmin(data);
        },
        (err) => {
          if (mounted.current) throw err;
        }
      );
      const done = quiet ? request.catch(() => {}) : errorToaster(toastContext, request);
      return done.finally(() => {
        if (mounted.current) setNow(Date.now());
      });
    },
    [toastContext]
  );

  React.useEffect(() => {
    mounted.current = true;
    load(false);
    return () => {
      mounted.current = false;
    };
  }, [load]);

  const checking = admin && admin.wallets.some((wallet) => isChecking(admin, wallet, now));
  const pollMs = checking ? CHECKING_POLL_MS : POLL_MS;
  React.useEffect(() => {
    const loadIfVisible = () => {
      if (document.visibilityState === "visible") load(true);
    };

    const timer = setInterval(loadIfVisible, pollMs);
    document.addEventListener("visibilitychange", loadIfVisible);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", loadIfVisible);
    };
  }, [load, pollMs]);

  const refresh = React.useCallback(() => load(false), [load]);
  return [admin, refresh, now];
}

// One line per wallet that says whether the check works. Every state carries a glyph or a word as
// well as a colour.
function StatusLine({ admin, wallet, now }) {
  const checkedAt = wallet.checked_at;
  const age = checkedAt ? now / 1000 - checkedAt : null;

  let variant = "success";
  let text;
  if (!admin.enabled) {
    variant = "warning";
    text = `${WARN} The SRP check is switched off in the server config.`;
  } else if (wallet.last_error) {
    variant = "danger";
    text = `✕ Not working: ${wallet.last_error}`;
  } else if (isChecking(admin, wallet, now)) {
    variant = "secondary";
    text = "◷ Checking the wallet now. The first check takes up to a minute.";
  } else if (age === null) {
    variant = "warning";
    text = `${WARN} No successful check yet. The server may not be running the SRP check.`;
  } else if (age > STALE_SECONDS) {
    variant = "warning";
    const span = formatSpan(age);
    text = `${WARN} No successful check for ${span}. The server may not be running the SRP check.`;
  } else {
    text = `✓ Working · last check ${formatClockUtc(new Date(checkedAt * 1000))} (${formatAgo(
      checkedAt,
      now
    )})`;
  }

  return <Status variant={variant}>{text}</Status>;
}

// Warnings about SRP not being shown anywhere and the [srp] managers setting in the server config
function ConfigNotes({ admin }) {
  const unknownManagers = admin.unknown_managers;
  // With no wallets or no active waitlists at all, the steps below say so themselves
  const none =
    admin.wallets.length > 0 &&
    admin.waitlists.length > 0 &&
    !admin.waitlists.some((w) => w.corporation_id !== null);
  return (
    <>
      {none && (
        <InfoNote variant="warning">
          <span>
            {WARN} SRP isn't shown on any waitlist yet. Pick an SRP corp for the waitlists below.
          </span>
        </InfoNote>
      )}
      {unknownManagers.length > 0 && (
        <InfoNote variant="warning">
          <span>
            {WARN} No character named {unknownManagers.map((name) => `"${name}"`).join(", ")} has
            logged in to the waitlist. Check [srp] managers in the server config, or ask them to log
            in.
          </span>
        </InfoNote>
      )}
    </>
  );
}

function RecentPayments({ admin, wallet }) {
  const recent = wallet.recent;
  return (
    <details>
      <Toggle>Recent payments ({recent.length})</Toggle>
      {recent.length === 0 ? (
        <p style={{ margin: "0.5em 0" }}>
          <em>No payments in the last {admin.coverage_hours} hours.</em>
        </p>
      ) : (
        <TableScroll>
          <Table fullWidth style={{ minWidth: "560px" }}>
            <TableHead>
              <Row>
                <CellHead>Character</CellHead>
                <CellHead>Amount</CellHead>
                <CellHead>Paid</CellHead>
                <CellHead>Covered until</CellHead>
                <CellHead>How</CellHead>
              </Row>
            </TableHead>
            <TableBody>
              {recent.map((payment) => (
                <Row key={`${payment.character.id}-${payment.paid_at}`}>
                  <Cell>{payment.character.name || payment.character.id}</Cell>
                  <Cell>{formatIsk(payment.amount)}</Cell>
                  <Cell>{datetime(payment.paid_at)}</Cell>
                  <Cell>{datetime(payment.expires_at)}</Cell>
                  <Cell>{payment.via === "contract" ? "Ship loan contract" : "Donation"}</Cell>
                </Row>
              ))}
            </TableBody>
          </Table>
        </TableScroll>
      )}
    </details>
  );
}

function Wallet({ admin, wallet, now, onChange }) {
  const toastContext = React.useContext(ToastContext);
  const [disconnectOpen, setDisconnectOpen] = React.useState(false);
  const [disconnecting, setDisconnecting] = React.useState(false);
  const uses = admin.waitlists.filter((w) => w.corporation_id === wallet.corporation_id).length;

  const confirmDisconnect = () => {
    if (disconnecting) return;
    setDisconnecting(true);
    toaster(toastContext, disconnect(wallet)).finally(() => {
      setDisconnecting(false);
      setDisconnectOpen(false);
      onChange();
    });
  };

  return (
    <WalletBlock>
      <WalletHead>
        <strong>{wallet.corporation_name}</strong>
        <Hint>
          used by {uses} {uses === 1 ? "waitlist" : "waitlists"}
        </Hint>
      </WalletHead>
      <StatusLine admin={admin} wallet={wallet} now={now} />
      <Details>
        <dt>Wallet character</dt>
        <dd>{wallet.character.name}</dd>
        <dt>Connected by</dt>
        <dd>
          {wallet.connected_by.name} · {datetime(wallet.connected_at)}
        </dd>
      </Details>
      <RecentPayments admin={admin} wallet={wallet} />
      <Buttons style={{ marginTop: "0.5em" }}>
        <Button variant="danger" disabled={disconnecting} onClick={() => setDisconnectOpen(true)}>
          {disconnecting ? "Disconnecting…" : "Disconnect"}
        </Button>
      </Buttons>
      <Confirm
        open={disconnectOpen}
        setOpen={setDisconnectOpen}
        title={`Disconnect the ${wallet.corporation_name} wallet`}
        onConfirm={confirmDisconnect}
      >
        <p style={{ margin: "0.5em 0 1em" }}>
          Disconnect the {wallet.corporation_name} wallet? Waitlists using it stop showing SRP until
          it's reconnected.
        </p>
      </Confirm>
    </WalletBlock>
  );
}

function ConnectWallet({ admin }) {
  const authContext = React.useContext(AuthContext);
  const [picked, setPicked] = React.useState(null);
  const [startError, setStartError] = React.useState(null);
  const [connecting, setConnecting] = React.useState(false);

  const characters = authContext.characters;
  const selected = picked === null ? authContext.current.id : picked;

  const connect = () => {
    setStartError(null);
    setConnecting(true);
    startConnect(selected).catch((err) => {
      setStartError(err.toString());
      setConnecting(false);
    });
  };

  return (
    <>
      <p style={{ margin: "0.5em 0" }}>
        Pick your character in the SRP corp that has the Junior Accountant role. It never needs to
        fly or log in again.
      </p>
      <Buttons>
        <Select
          aria-label="Wallet character"
          value={selected}
          disabled={connecting}
          onChange={(evt) => setPicked(parseInt(evt.target.value, 10))}
        >
          {characters.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </Select>
        <Button variant="primary" disabled={connecting || !admin.enabled} onClick={connect}>
          {connecting ? "Connecting…" : "Connect"}
        </Button>
        {!admin.enabled && <Hint>Switch the SRP check on in the server config to connect.</Hint>}
      </Buttons>
      {startError && <InfoNote variant="danger">{startError}</InfoNote>}
    </>
  );
}

function Wallets({ admin, now, onChange }) {
  return (
    <Step aria-labelledby="srp-step-wallets">
      <StepHeading id="srp-step-wallets">
        <StepNumber aria-hidden="true">1</StepNumber>SRP wallets
      </StepHeading>
      <Hint>
        One per SRP corp. Connecting a character adds its corp's wallet, or replaces it if that corp
        is already connected.
      </Hint>
      {admin.wallets.length === 0 ? (
        <Muted style={{ margin: "0.8em 0" }}>No SRP wallet connected yet.</Muted>
      ) : (
        admin.wallets.map((wallet) => (
          <Wallet
            key={wallet.corporation_id}
            admin={admin}
            wallet={wallet}
            now={now}
            onChange={onChange}
          />
        ))
      )}
      <ConnectWallet admin={admin} />
    </Step>
  );
}

async function setWaitlist(waitlistId, corporationId) {
  await apiCall("/api/srp/admin/waitlist", {
    json: { waitlist_id: waitlistId, corporation_id: corporationId },
  });
}

// Picking a corp saves straight away. Each select is disabled while its own request runs, and the
// value always comes from the server's answer, so a failed save leaves it as it was.
function WaitlistPicker({ admin, onChange }) {
  const toastContext = React.useContext(ToastContext);
  const [pending, setPending] = React.useState([]);
  const { waitlists, wallets } = admin;

  const pick = (waitlist, value) => {
    const corporationId = value === "" ? null : parseInt(value, 10);
    const wallet = wallets.find((w) => w.corporation_id === corporationId);
    setPending((ids) => [...ids, waitlist.id]);
    const request = setWaitlist(waitlist.id, corporationId).then(() =>
      addToast(toastContext, {
        message: wallet
          ? `${waitlist.name} now uses ${wallet.corporation_name}`
          : `SRP removed from ${waitlist.name}`,
        variant: "success",
      })
    );
    errorToaster(toastContext, request)
      .then(onChange)
      .finally(() => setPending((ids) => ids.filter((id) => id !== waitlist.id)));
  };

  return (
    <Step aria-labelledby="srp-step-waitlists">
      <StepHeading id="srp-step-waitlists">
        <StepNumber aria-hidden="true">2</StepNumber>Waitlists
      </StepHeading>
      <Hint>
        {wallets.length === 0
          ? "Connect a wallet above first, then pick which SRP corp each waitlist uses."
          : "Pick which SRP corp each waitlist uses. Pilots are covered by payments to that corp only. Changes save straight away; pilots see them next time they open the waitlist."}
      </Hint>
      {waitlists.length === 0 ? (
        <p style={{ margin: "0.8em 0" }}>
          <em>There are no active waitlists.</em>
        </p>
      ) : (
        <Fieldset style={{ marginTop: "0.5em" }}>
          <HiddenLegend>SRP corp for each waitlist</HiddenLegend>
          {waitlists.map((waitlist) => {
            const corpId = waitlist.corporation_id;
            const wallet = wallets.find((w) => w.corporation_id === corpId);
            // Its wallet was disconnected after the waitlist picked it
            const orphaned = corpId !== null && !wallet;
            const byId = `srp-wl-${waitlist.id}-by`;
            const showBy = wallet && waitlist.set_by && waitlist.set_at;
            return (
              <WaitlistRow key={waitlist.id}>
                <label className="name" htmlFor={`srp-wl-${waitlist.id}`}>
                  {waitlist.name}
                </label>
                <div className="pick">
                  <Select
                    id={`srp-wl-${waitlist.id}`}
                    value={corpId === null ? "" : String(corpId)}
                    disabled={pending.includes(waitlist.id)}
                    aria-describedby={showBy ? byId : undefined}
                    onChange={(evt) => pick(waitlist, evt.target.value)}
                  >
                    <option value="">No SRP</option>
                    {wallets.map((w) => (
                      <option key={w.corporation_id} value={String(w.corporation_id)}>
                        {w.corporation_name}
                      </option>
                    ))}
                    {orphaned && <option value={String(corpId)}>&lt;disconnected corp&gt;</option>}
                  </Select>
                </div>
                {showBy && (
                  <Hint as="div" id={byId} className="note">
                    by {waitlist.set_by.name} · {datetime(waitlist.set_at)}
                  </Hint>
                )}
                {orphaned && (
                  <div className="note">
                    <Status variant="warning" style={{ marginBottom: 0 }}>
                      {WARN} Its wallet isn't connected, so pilots see nothing.
                    </Status>
                  </div>
                )}
              </WaitlistRow>
            );
          })}
        </Fieldset>
      )}
    </Step>
  );
}

// The rule the check applies, plus the time zone of every time on this page
function Footnote({ admin }) {
  return (
    <Muted style={{ marginTop: "1.5em", fontSize: "0.9em" }}>
      Counting payments of {formatIsk(admin.min_amount)}+ to the SRP corp, covering the paying
      character for {admin.coverage_hours} hours (wallet division {admin.division}). Times are EVE
      time (UTC).
    </Muted>
  );
}

export function SrpWallet() {
  const [admin, refresh, now] = useSrpAdmin();

  return (
    <>
      <PageTitle>SRP wallet</PageTitle>
      <Content style={{ maxWidth: "900px" }}>
        {!admin ? (
          <em>Loading SRP wallet...</em>
        ) : (
          <>
            {admin.connect_error && <InfoNote variant="danger">{admin.connect_error}</InfoNote>}
            <ConfigNotes admin={admin} />
            <Wallets admin={admin} now={now} onChange={refresh} />
            <WaitlistPicker admin={admin} onChange={refresh} />
            <Footnote admin={admin} />
          </>
        )}
      </Content>
    </>
  );
}
