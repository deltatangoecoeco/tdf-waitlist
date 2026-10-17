import React from "react";
import { Route, Routes } from "react-router-dom";
import { AuthContext } from "../../contexts";
import { BanRoutes } from "./Bans";
import { Fleet, FleetRegister } from "./Fleet";
import { Search } from "./Search";
// import { Statistics } from "./Statistics";
import { FleetCompHistory } from "./FleetCompHistory";
import { NoteAdd } from "./NoteAdd";
import { Announcements } from "./Announcements";
import { FCMenu, GuideFC } from "./FCMenu";
import { View as BadgesView } from "./Badges";
import { View as CommandersView } from "./Commanders";
import { SrpWallet } from "./SrpWallet";

const Statistics = React.lazy(() => import("./lazy.statistics"));

export function FCRoutes() {
  const authContext = React.useContext(AuthContext);
  return (
    <Routes>
      <Route path="bans/*" element={<BanRoutes />} />
      
      {authContext && authContext.access["badges-manage"] && (
        <Route path="badges" element={<BadgesView />} />
      )}

      {authContext && authContext.access["access-manage"] && (
        <Route path="commanders" element={<CommandersView />} />
      )}

      {authContext && authContext.access["srp-manage"] && (
        <Route path="srp" element={<SrpWallet />} />
      )}

      <Route path="/" element={<FCMenu />} />
      
      <Route path="fleet" element={<Fleet />} />
      <Route path="fleet/register" element={<FleetRegister />} />
      <Route path="search" element={<Search />} />
      <Route path="fleet-comp-history" element={<FleetCompHistory />} />
      <Route path="notes/add" element={<NoteAdd />} />
      {authContext.access["stats-view"] && (
        <>
        <Route path="stats" element={<Statistics />} />
        <Route path="documentation" element={<GuideFC />} />
        </>
      )}
      {authContext.access["fleet-view"] && (
        <Route path="trainee" element={<GuideFC />} />
      )}
      {authContext.access["waitlist-tag:HQ-FC"] && (
        <Route path="/fc/announcement" element={<Announcements />} />
      )}
    </Routes>
  );
}
