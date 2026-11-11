import React from "react";
import type { Workspace } from "@/lib/types";
import { cn, shortenHomePath } from "@/lib/utils";
import { Btn, Icon, Seg } from "@/ui";

/** 顶栏只放：品牌、项目名、模式（CLI / API）、额度与花费、设置。 */
export const TopBar: React.FC<{
  native: boolean;
  project: Workspace | null;
  mode: "cli" | "api";
  onMode: (mode: "cli" | "api") => void;
  page: "" | "settings" | "quota";
  onPage: (page: "settings" | "quota") => void;
}> = ({ native, project, mode, onMode, page, onPage }) => (
  <header className={cn("app-top", native && "app-top--native")} data-tauri-drag-region>
    <div className="brand" data-tauri-drag-region>
      <b>PECK</b>
      <i className="brand-mark" />
    </div>
    <span className="top-project" data-tauri-drag-region>
      <Icon name="folder" />
      {project ? (
        <>
          <b className="t-name">{project.name}</b>
          <em className="t-code">{shortenHomePath(project.path)}{project.gitBranch ? ` · ${project.gitBranch}` : ""}</em>
        </>
      ) : (
        <b className="t-name top-project-none">还没选项目</b>
      )}
    </span>
    <span className="grow" data-tauri-drag-region />
    <Seg label="模式" sm items={[{ id: "cli", label: "CLI" }, { id: "api", label: "API" }]} value={mode} onChange={onMode} />
    <Btn v="ghost" className={cn(page === "quota" && "is-here")} aria-label="额度与花费" aria-pressed={page === "quota"} onClick={() => onPage("quota")}>QUOTA</Btn>
    <Btn v="ghost" square className={cn("top-gear", page === "settings" && "is-here")} aria-label="设置" aria-pressed={page === "settings"} onClick={() => onPage("settings")}>
      <Icon name="gear" />
    </Btn>
  </header>
);
