import React from "react";
import { AuthContext } from "../contexts";
import { apiCall } from "../api";
import styled from "styled-components";
import { NavButton } from "./Form";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faTriangleExclamation } from "@fortawesome/free-solid-svg-icons";
import { useTranslation } from "react-i18next";
import { isRegisteredBoss } from "../Util/fleet";

const CHECK_INTERVAL_MS = 15000;

// Not InfoNote: its solid warning background (plus drop shadow) is loud, and text on it is hard to
// read in the dark themes. Instead: the panel colour lightly tinted with the warning colour and a
// warning-coloured border, so it stands out while the theme's own text colour stays readable.
const Banner = styled.div`
  display: flex;
  gap: 0.6em;
  margin: 0 0 1em;
  padding: 0.6em 0.8em;
  background-color: ${(props) => props.theme.colors.accent1};
  background-color: color-mix(
    in srgb,
    ${(props) => props.theme.colors.warning.color} 22%,
    ${(props) => props.theme.colors.accent1}
  );
  border: solid 1px ${(props) => props.theme.colors.warning.color};
  border-left-width: 6px;
  border-radius: 5px;
`;

// Solid page background: the default button fill is translucent and disappears into the tint
const RegisterButton = styled(NavButton)`
  && {
    background-color: ${(props) => props.theme.colors.background};
  }
  &&:hover {
    background-color: ${(props) => props.theme.colors.accent1};
  }
`;

// Warns an FC who is the in-game boss of a fleet that the waitlist is not tracking. That
// happens after a boss handover or a new fleet: the poller loses the old boss's token and
// drops the registration, and until someone re-registers invites and removals do nothing.
export function UnregisteredFleetBanner() {
  const authContext = React.useContext(AuthContext);
  const { t } = useTranslation("fleet");
  const [unregistered, setUnregistered] = React.useState(false);
  const canConfigure = !!(
    authContext &&
    authContext.access &&
    authContext.access["fleet-configure"]
  );
  const characterId = authContext && authContext.current ? authContext.current.id : null;

  React.useEffect(() => {
    if (!canConfigure || characterId === null) {
      setUnregistered(false);
      return;
    }

    let cancelled = false;

    const check = async () => {
      let result = false;
      try {
        const me = await apiCall("/api/fleet/me?character_id=" + characterId, {});
        if (me.is_boss) {
          const status = await apiCall("/api/fleet/status", {});
          // Match the boss as well as the fleet id: right after a handover the old
          // registration still exists for a few seconds, under the previous FC.
          result = !isRegisteredBoss(status.fleets, characterId, me.fleet_id);
        }
      } catch (err) {
        // Not in a fleet (404) or anything else going wrong: stay hidden
      }
      if (!cancelled) setUnregistered(result);
    };

    const checkIfVisible = () => {
      if (document.visibilityState === "visible") check();
    };

    check();
    const timer = setInterval(checkIfVisible, CHECK_INTERVAL_MS);
    document.addEventListener("visibilitychange", checkIfVisible);
    return () => {
      cancelled = true;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", checkIfVisible);
    };
  }, [canConfigure, characterId]);

  if (!canConfigure || !unregistered) {
    return null;
  }

  return (
    <Banner role="status">
      <FontAwesomeIcon icon={faTriangleExclamation} style={{ marginTop: "0.2em" }} />
      <div>
        <strong>{t("unregistered_title")}</strong>
        <br />
        {t("unregistered_text")}
        <div style={{ marginTop: "0.5em" }}>
          <RegisterButton to="/fc/fleet/register">{t("unregistered_button")}</RegisterButton>
        </div>
      </div>
    </Banner>
  );
}
