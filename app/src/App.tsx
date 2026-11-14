import { useEffect, useState } from "react";

import { AlarmWatcher, Banners, Toasts } from "@/components/Banners.tsx";
import { ConnectDialog } from "@/components/Dialogs.tsx";
import { PresenceWatcher } from "@/components/PresenceWatcher.tsx";
import { TopBar } from "@/components/TopBar.tsx";
import type { Screen } from "@/state/helpers.ts";
import { ArchiveScreen } from "@/screens/ArchiveScreen.tsx";
import { CareScreen } from "@/screens/CareScreen.tsx";
import { InventoryView } from "@/screens/InventoryView.tsx";
import { ProfilesScreen } from "@/screens/ProfilesScreen.tsx";
import { RoastScreen } from "@/screens/RoastScreen.tsx";
import { SaveWatcher } from "@/state/actions.ts";
import { refreshCare } from "@/state/care.ts";
import { startLive, useLive } from "@/state/live.ts";

export function App() {
  const { snapshot } = useLive();
  const [screen, setScreen] = useState<Screen>("roast");
  const [connectOpen, setConnectOpen] = useState(false);

  useEffect(() => startLive(), []);
  useEffect(() => refreshCare(), []);

  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      <TopBar
        snapshot={snapshot}
        screen={screen}
        onScreen={setScreen}
        onConnect={() => setConnectOpen(true)}
      />
      <Banners />
      {screen === "roast" ? (
        <RoastScreen onConnect={() => setConnectOpen(true)} />
      ) : screen === "inventory" ? (
        <main className="flex min-h-0 flex-1 flex-col overflow-y-auto p-gutter lg:overflow-hidden">
          <InventoryView />
        </main>
      ) : screen === "archive" ? (
        <ArchiveScreen />
      ) : screen === "care" ? (
        <CareScreen />
      ) : (
        <ProfilesScreen />
      )}
      <ConnectDialog open={connectOpen} onOpenChange={setConnectOpen} />
      <AlarmWatcher />
      <PresenceWatcher />
      <SaveWatcher />
      <Toasts />
    </div>
  );
}
