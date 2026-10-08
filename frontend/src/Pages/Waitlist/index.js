import React from "react";
import { useLocation } from "react-router";
import { AuthContext, /*ToastContext, */ EventContext, WaitlistContext } from "../../contexts";
import { apiCall, /*errorToaster, */ useApi } from "../../api";
import { InputGroup, Button, Buttons, NavButton } from "../../Components/Form";
// import { InfoAnnouncement } from "../../Components/Announcement";
import {
  ColumnWaitlist,
  CompactWaitlist,
  LinearWaitlist,
  MatrixWaitlist,
  RowWaitlist,
  NotepadWaitlist,
  CategoryHeading,
} from "./displaymodes";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faColumns } from "@fortawesome/free-solid-svg-icons";
import { useQuery } from "../../Util/query";
import { useTranslation } from "react-i18next";
import { UnregisteredFleetBanner } from "../../Components/UnregisteredFleetBanner";

function coalesceCalls(func, wait) {
  var nextCall = null;
  var timer = null;

  const timerFn = function () {
    timer = setTimeout(timerFn, wait);

    if (nextCall) {
      const [context, args] = nextCall;
      nextCall = null;
      func.apply(context, args);
    }
  };

  // Splay the initial timer, after that use a constant time interval
  timer = setTimeout(timerFn, wait * Math.random());

  return [
    function () {
      nextCall = [this, arguments];
    },
    function () {
      clearTimeout(timer);
    },
  ];
}

// async function removeEntry(id) {
//   return await apiCall("/api/waitlist/remove_x", {
//     json: { id },
//   });
// }

function useWaitlist(waitlistId) {
  const eventContext = React.useContext(EventContext);

  const [waitlistData, refreshFn] = useApi(
    waitlistId ? `/api/waitlist?waitlist_id=${waitlistId}` : null
  );

  // Listen for events
  React.useEffect(() => {
    if (!eventContext) return;

    const [updateFn, clearUpdateFn] = coalesceCalls(refreshFn, 2000);
    const handleEvent = function (event) {
      var data = JSON.parse(event.data);
      if (data.waitlist_id === waitlistId) {
        updateFn();
      }
    };
    eventContext.addEventListener("waitlist_update", handleEvent);
    eventContext.addEventListener("open", updateFn);
    return function () {
      clearUpdateFn();
      eventContext.removeEventListener("waitlist_update", handleEvent);
      eventContext.removeEventListener("open", updateFn);
    };
  }, [refreshFn, eventContext, waitlistId]);

  return [waitlistData, refreshFn];
}

function useFleetComposition() {
  const authContext = React.useContext(AuthContext);
  const eventContext = React.useContext(EventContext);
  const [fleetMembers, setFleetMembers] = React.useState(null);

  const refreshFn = React.useCallback(async () => {
    if (!authContext.access["fleet-view"]) {
      setFleetMembers(null);
      return;
    }

    try {
      const fleets = await apiCall("/api/fleet/status", {});

      if (
        !fleets ||
        fleets.fleets.length === 0 ||
        !fleets.fleets.some((n) => authContext.current.id === n.boss.id)
      ) {
        setFleetMembers(null);
        return;
      }

      const fleetMembers = await apiCall(
        `/api/fleet/members?character_id=${authContext.current.id}`,
        {}
      );
      setFleetMembers(fleetMembers);
    } catch (e) {
      setFleetMembers(null);
    }

  }, [authContext, setFleetMembers]);

  React.useEffect(() => {
    refreshFn();
  }, [refreshFn]);

  React.useEffect(() => {
    if (!eventContext) return;

    const [updateFn, clearUpdateFn] = coalesceCalls(refreshFn, 2000);
    eventContext.addEventListener("comp_update", updateFn);
    eventContext.addEventListener("open", updateFn);
    return function () {
      clearUpdateFn();
      eventContext.removeEventListener("comp_update", updateFn);
      eventContext.removeEventListener("open", updateFn);
    };
  }, [refreshFn, eventContext]);

  return fleetMembers;
}

export function Waitlist() {
  const authContext = React.useContext(AuthContext);
  // const toastContext = React.useContext(ToastContext);
  const waitlistContext = React.useContext(WaitlistContext);
  const [query, setQuery] = useQuery();
  const waitlistId = waitlistContext !== null ? waitlistContext.active : null;
  const [altCol, setAltCol] = React.useState(
    window.localStorage && window.localStorage.getItem("AltColumn")
      ? window.localStorage.getItem("AltColumn") === "true"
      : false
  );
  const [waitlistData, refreshWaitlist] = useWaitlist(waitlistId);
  const fleetComposition = useFleetComposition();
  const { t } = useTranslation("waitlist");
  const displayMode = query.mode || "columns";
  const location = useLocation();

  React.useEffect(() => {
    if (waitlistId !== null) {
      const params = new URLSearchParams(location.search);
      params.set("wl", waitlistId);
      window.history.replaceState({}, "", `${location.pathname}?${params.toString()}`);
    }
  }, [waitlistId, location]);

  const setDisplayMode = (newMode) => {
    setQuery("mode", newMode);
  };

  if (!waitlistId) {
    return null; // Should be redirecting
  }

  if (waitlistData === null) {
    return <em>{t("loading")}</em>;
  }
  if (!waitlistData.open) {
    return (
      <>
        {/* An FC forming a new fleet usually registers it before opening the waitlist */}
        <UnregisteredFleetBanner />
        {/* <InfoAnnouncement id={2} /> */}
        <em>{t("notopen")}</em>
      </>
    );
  }
  const handleChange = () => {
    setAltCol(!altCol);
    if (window.localStorage) {
      window.localStorage.setItem("AltColumn", !altCol);
    }
  };

  const myEntry = waitlistData.waitlist.find(
    (ent) =>
      ent &&
      ent.fits &&
      ent.fits.some((fit) => fit && fit.character && fit.character.id === authContext.current.id)
  );

  return (
    <>
      <UnregisteredFleetBanner />
      {/* <InfoAnnouncement id={2} /> */}
      <Buttons>
        <InputGroup>
          <NavButton variant={myEntry ? null : "primary"} to={`/xup?wl=${waitlistId}`}>
            {myEntry ? t("update_fit") : t("join")}
          </NavButton>
          {/* <Button
            variant={myEntry ? "danger" : null}
            onClick={(evt) => errorToaster(toastContext, removeEntry(myEntry.id))}
            disabled={myEntry ? false : true}
          >
            {t("leave")}
          </Button> */}
        </InputGroup>
        <InputGroup>
          <Button active={displayMode === "columns"} onClick={(evt) => setDisplayMode("columns")}>
            {t("columns")}
          </Button>
          <Button active={displayMode === "matrix"} onClick={(evt) => setDisplayMode("matrix")}>
            {t("matrix")}
          </Button>
          <Button active={displayMode === "compact"} onClick={(evt) => setDisplayMode("compact")}>
            {t("compact")}
          </Button>
          <Button active={displayMode === "linear"} onClick={(evt) => setDisplayMode("linear")}>
            {t("linear")}
          </Button>
          <Button active={displayMode === "rows"} onClick={(evt) => setDisplayMode("rows")}>
            {t("rows")}
          </Button>
          {authContext.access["waitlist-view"] && (
              <Button active={displayMode === "notepad"} onClick={(evt) => setDisplayMode("notepad")}>
                {t("notepad")}
              </Button>
          )}
        </InputGroup>
        {(displayMode === "columns" || displayMode === "rows" || displayMode === "matrix") && (
          <InputGroup>
            <Button onClick={handleChange}>
              <FontAwesomeIcon icon={faColumns} />
            </Button>
          </InputGroup>
        )}
        {!altCol && (
          <CategoryHeading name="小号 ALT" fleetComposition={fleetComposition} altCol={altCol} />
        )}
      </Buttons>

      {displayMode === "columns" ? (
        <ColumnWaitlist
          waitlist={waitlistData}
          onAction={refreshWaitlist}
          fleetComposition={fleetComposition}
          altCol={altCol}
        />
      ) : displayMode === "compact" ? (
        <CompactWaitlist waitlist={waitlistData} onAction={refreshWaitlist} />
      ) : displayMode === "linear" ? (
        <LinearWaitlist waitlist={waitlistData} onAction={refreshWaitlist} />
      ) : displayMode === "matrix" ? (
        <MatrixWaitlist
          waitlist={waitlistData}
          onAction={refreshWaitlist}
          fleetComposition={fleetComposition}
          altCol={altCol}
        />
      ) : displayMode === "rows" ? (
        <RowWaitlist
          waitlist={waitlistData}
          onAction={refreshWaitlist}
          fleetComposition={fleetComposition}
          altCol={altCol}
        />
      ) : displayMode === "notepad" ? (
        <NotepadWaitlist waitlist={waitlistData} onAction={refreshWaitlist} />
      ) : null}
    </>
  );
}
