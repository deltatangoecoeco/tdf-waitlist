import React from "react";
import styled from "styled-components";
import { Trans, useTranslation } from "react-i18next";
import { AuthContext } from "../../contexts";
import { apiCall } from "../../api";
import { daysFromToday, formatClock, formatIsk } from "../../Util/srp";

const POLL_MS = 60000;

// Fetches the SRP status of the given characters on the given waitlist and keeps it fresh while the
// tab is visible. Returns null whenever we don't know (feature off, no SRP corp for this waitlist,
// wallet not readable, request failed): the caller then shows nothing, so a pilot is never told
// they haven't paid when we can't tell. Stops polling once the server says SRP will never apply
// here, until the waitlist or the characters change.
function useSrpStatus(characterIds, waitlistId) {
  const [status, setStatus] = React.useState(null);
  const idsParam = characterIds.join(",");

  React.useEffect(() => {
    // Never show the previous waitlist's or characters' data while the new request is out
    setStatus(null);
    if (!idsParam || !waitlistId) return;

    let cancelled = false;
    let timer = null;
    const stop = () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", loadIfVisible);
    };
    const load = () => {
      apiCall(`/api/srp/status?character_ids=${idsParam}&waitlist_id=${waitlistId}`, {}).then(
        (data) => {
          if (cancelled) return;
          setStatus(data && data.available ? data : null);
          if (data && data.applies === false) stop();
        },
        () => {
          if (!cancelled) setStatus(null);
        }
      );
    };
    function loadIfVisible() {
      if (document.visibilityState === "visible") load();
    }

    load();
    timer = setInterval(loadIfVisible, POLL_MS);
    document.addEventListener("visibilitychange", loadIfVisible);
    return () => {
      cancelled = true;
      stop();
    };
  }, [idsParam, waitlistId]);

  return status;
}

// Sits at the right-hand end of the waitlist's Buttons row; `&&` beats the row's own margins
const Bar = styled.div`
  && {
    margin-left: auto;
    margin-right: 0;
  }
  position: relative;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
  max-width: 100%;
`;

// The "SRP" label and its info icon
const LabelGroup = styled.span`
  display: inline-flex;
  align-items: center;
  gap: 4px;
`;

const Label = styled.span`
  font-weight: 700;
  font-size: 0.9em;
  color: ${(props) => props.theme.colors.text};
`;

const InfoButton = styled.button`
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: none;
  color: ${(props) => props.theme.colors.accent4};
  cursor: pointer;
  &:hover,
  &[aria-expanded="true"] {
    color: ${(props) => props.theme.colors.text};
  }
  &:focus-visible {
    outline: 2px solid ${(props) => props.theme.colors.active};
    outline-offset: 2px;
  }
`;

function InfoIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
      <circle cx="8" cy="8" r="6.7" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <circle cx="8" cy="4.9" r="0.95" fill="currentColor" />
      <path d="M8 7.3v4.2" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

// Paid keeps the normal text colour: white on the success green fails contrast on the Light theme
// and so does green text, so the green lives in the border and the check mark only. It is a button
// so that phones, which have no hover, can tap it to open the popover.
const Chip = styled.button`
  box-sizing: border-box;
  max-width: 100%;
  margin: 0;
  font-family: inherit;
  line-height: inherit;
  text-align: left;
  cursor: pointer;
  appearance: none;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 0.85em;
  padding: 0.3em 0.7em;
  border-radius: 4px;
  background-color: ${(props) => props.theme.colors.background};
  color: ${(props) => (props.paid ? props.theme.colors.text : props.theme.colors.accent4)};
  border: 1px solid
    ${(props) => (props.paid ? props.theme.colors.success.color : props.theme.colors.accent2)};
  ${(props) => props.paid && "font-weight: 600;"}
  &:focus-visible {
    outline: 2px solid ${(props) => props.theme.colors.active};
    outline-offset: 2px;
  }
`;

const Check = styled.span`
  color: ${(props) => props.theme.colors.success.color};
`;

// `left`, `width` and the caret position come in as inline styles: they depend on which trigger
// opened it and how wide the window is
const Popover = styled.div`
  position: absolute;
  z-index: 20;
  top: calc(100% + 9px);
  padding: 0.7em 0.9em;
  font-size: 0.9em;
  line-height: 1.5;
  text-align: left;
  background-color: ${(props) => props.theme.colors.background};
  color: ${(props) => props.theme.colors.text};
  border: 1px solid ${(props) => props.theme.colors.accent2};
  border-radius: 6px;
  box-shadow: 0 6px 16px ${(props) => props.theme.colors.shadow};

  &::before {
    content: "";
    position: absolute;
    top: -6px;
    left: calc(var(--caret) - 5px);
    width: 10px;
    height: 10px;
    background-color: ${(props) => props.theme.colors.background};
    border-top: 1px solid ${(props) => props.theme.colors.accent2};
    border-left: 1px solid ${(props) => props.theme.colors.accent2};
    transform: rotate(45deg);
  }
  p + p,
  h4 + ul,
  ul + p {
    margin-top: 0.4em;
  }
  h4 {
    margin-top: 0.7em;
    font-size: 0.9em;
    font-weight: 700;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    overflow-wrap: anywhere;
  }
  b {
    font-weight: 700;
    color: ${(props) => props.theme.colors.active};
  }
  .muted {
    color: ${(props) => props.theme.colors.accent4};
  }
`;

const POPOVER_WIDTH = 330;
const VIEWPORT_GUTTER = 8;

// Where to put the popover so it hangs under the trigger that opened it without leaving the
// viewport: its width, its left edge as an offset from the bar, and the caret's offset from its own
// left edge.
function popoverPlacement(trigger, bar) {
  const viewport = document.documentElement.clientWidth;
  const width = Math.min(POPOVER_WIDTH, viewport - 2 * VIEWPORT_GUTTER);
  const box = trigger.getBoundingClientRect();
  const center = box.left + box.width / 2;
  const maxLeft = viewport - width - VIEWPORT_GUTTER;
  const left = Math.max(VIEWPORT_GUTTER, Math.min(center - 24, maxLeft));
  const caret = Math.max(14, Math.min(center - left, width - 14));
  return { width, left: left - bar.getBoundingClientRect().left, caret };
}

function coveredUntil(t, language, expiresAt) {
  const date = new Date(expiresAt * 1000);
  const time = formatClock(date);
  const days = daysFromToday(date);
  if (days === 0) return t("srp_today", { time });
  if (days === 1) return t("srp_tomorrow", { time });
  return t("srp_date_time", { date: date.toLocaleDateString(language), time });
}

// One line per paid character. Unpaid characters are deliberately never named, in the tooltip or in
// the popover. Each key has a `_contract` variant: the whole sentence, so translators can place the
// ship loan contract where their language needs it.
function paidLine(t, language, key, name, entry) {
  const variant = entry.via === "contract" ? `${key}_contract` : key;
  return t(variant, { name, until: coveredUntil(t, language, entry.expires_at) });
}

// A single summary chip: how many of the account's characters are covered by SRP right now
export function SrpStatus({ waitlistId }) {
  const authContext = React.useContext(AuthContext);
  const { t, i18n } = useTranslation("waitlist");
  const [open, setOpen] = React.useState(false);
  // Which trigger opened the popover: the info icon or the chip. Focus and the caret follow it.
  const [anchor, setAnchor] = React.useState("info");
  const [placement, setPlacement] = React.useState({ width: POPOVER_WIDTH, left: 0, caret: 24 });
  const barRef = React.useRef(null);
  const infoRef = React.useRef(null);
  const chipRef = React.useRef(null);
  const anchorRef = anchor === "chip" ? chipRef : infoRef;

  const characters = React.useMemo(
    () => (authContext && authContext.characters) || [],
    [authContext]
  );
  const status = useSrpStatus(
    React.useMemo(() => characters.map((c) => c.id), [characters]),
    waitlistId
  );

  // Close on Escape or a click anywhere outside the bar and the popover
  React.useEffect(() => {
    if (!open) return;

    const onMouseDown = (evt) => {
      if (barRef.current && !barRef.current.contains(evt.target)) setOpen(false);
    };
    const onKeyDown = (evt) => {
      if (evt.key === "Escape") {
        setOpen(false);
        if (anchorRef.current) anchorRef.current.focus();
      }
    };
    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open, anchorRef]);

  // Keep the popover on screen: place it when it opens and again whenever the window resizes
  React.useLayoutEffect(() => {
    if (!open) return;

    const place = () => {
      if (anchorRef.current && barRef.current) {
        setPlacement(popoverPlacement(anchorRef.current, barRef.current));
      }
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open, anchorRef]);

  if (!status) return null;

  const byId = {};
  status.characters.forEach((entry) => {
    byId[entry.id] = entry;
  });
  const rows = characters.filter((c) => byId[c.id]);
  if (rows.length === 0) return null;

  const paidRows = rows.filter((c) => byId[c.id].paid);
  const anyPaid = paidRows.length > 0;
  const text =
    rows.length === 1
      ? t(anyPaid ? "srp_single_paid" : "srp_single_unpaid")
      : t("srp_summary_paid", { n: paidRows.length });
  const title = anyPaid
    ? paidRows
        .map((c) => paidLine(t, i18n.language, "srp_paid_line", c.name, byId[c.id]))
        .join("\n")
    : t("srp_unpaid_hint", { hours: status.coverage_hours });

  const amount = formatIsk(status.min_amount);
  const checked = status.checked_at && formatClock(new Date(status.checked_at * 1000));
  const popoverStyle = {
    width: placement.width,
    left: placement.left,
    "--caret": `${placement.caret}px`,
  };
  const toggle = (which) => {
    setAnchor(which);
    setOpen((o) => !o);
  };

  return (
    <Bar ref={barRef}>
      <LabelGroup>
        <Label>{t("srp_label")}</Label>
        <InfoButton
          type="button"
          ref={infoRef}
          aria-label={t("srp_info_label")}
          aria-expanded={open && anchor === "info"}
          aria-controls={open ? "srp-how-to-pay" : undefined}
          onClick={() => toggle("info")}
        >
          <InfoIcon />
        </InfoButton>
      </LabelGroup>
      <Chip
        type="button"
        ref={chipRef}
        paid={anyPaid}
        title={title}
        aria-expanded={open && anchor === "chip"}
        aria-controls={open ? "srp-how-to-pay" : undefined}
        onClick={() => toggle("chip")}
      >
        {anyPaid && (
          <>
            <Check aria-hidden="true">✓</Check>{" "}
          </>
        )}
        {text}
      </Chip>
      {open && (
        <Popover
          id="srp-how-to-pay"
          role="region"
          aria-label={t("srp_popover_label")}
          style={popoverStyle}
        >
          <p>
            <b>{t("srp_pay_lead")}</b>
          </p>
          <p>
            <Trans t={t} i18nKey="srp_pay_1" values={{ amount }} components={{ b: <b /> }} />
          </p>
          <p>{t("srp_pay_2", { hours: status.coverage_hours })}</p>
          {anyPaid && (
            <>
              <h4>
                {rows.length === 1
                  ? t("srp_covered_single")
                  : t("srp_covered", { paid: paidRows.length, total: rows.length })}
              </h4>
              <ul>
                {paidRows.map((c) => (
                  <li key={c.id}>
                    {paidLine(t, i18n.language, "srp_covered_line", c.name, byId[c.id])}
                  </li>
                ))}
              </ul>
            </>
          )}
          {checked && <p className="muted">{t("srp_pay_delay", { time: checked })}</p>}
        </Popover>
      )}
    </Bar>
  );
}
