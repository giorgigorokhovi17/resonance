import { Download, RefreshCw } from "lucide-react";
import { SecondaryButton } from "../../components/EmptyState";
import { useUpdater } from "../../store/updater";
import { Section } from "../SettingsView";

export function UpdatesSection() {
  const { phase, current, update, error, progress, check, install } = useUpdater();
  const busy = phase === "checking" || phase === "downloading" || phase === "installing";
  let status: string;
  switch (phase) {
    case "checking":
      status = "Checking for updates…";
      break;
    case "none":
      status = "You're on the latest version.";
      break;
    case "available":
      status = `Version ${update?.version} is available.`;
      break;
    case "downloading":
      status = `Downloading ${update?.version}${progress !== null ? ` (${Math.round(progress * 100)}%)` : "…"}`;
      break;
    case "installing":
      status = "Installing… Resonance will restart.";
      break;
    case "error":
      status = `Couldn't check for updates: ${error}`;
      break;
    default:
      status = "Resonance checks for updates automatically when it starts.";
  }
  return (
    <Section title="Updates" description={`Version ${current || "?"}. Updates are signed and installed in place; your library and settings are kept.`}>
      <div className="flex flex-wrap items-center gap-3">
        {phase === "available" ? (
          <SecondaryButton onClick={install}>
            <Download size={15} /> Update to {update?.version} and restart
          </SecondaryButton>
        ) : (
          <SecondaryButton onClick={() => !busy && check(false)}>
            <RefreshCw size={15} className={phase === "checking" ? "animate-spin" : ""} /> Check for updates
          </SecondaryButton>
        )}
        <span className={`text-[12.5px] ${phase === "error" ? "text-warn" : "text-muted"}`}>{status}</span>
      </div>
    </Section>
  );
}
