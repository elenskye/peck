import React from "react";
import type { Workspace } from "@/lib/types";
import { cn, shortenHomePath } from "@/lib/utils";
import { Btn, Icon } from "@/ui";

/** 收起 / 展开左栏、右栏（v1.1.12）：图标里那一条实心是栏开着，空心是收起了。 */
const SideBtn: React.FC<{ side: "left" | "right"; shown: boolean; onToggle: () => void }> = ({ side, shown, onToggle }) => {
  const name = side === "left" ? "左栏" : "右栏";
  return (
    <Btn v="ghost" square className="top-side" aria-label={`${shown ? "收起" : "展开"}${name}`} title={`${shown ? "收起" : "展开"}${name}`} aria-pressed={shown} onClick={onToggle}>
      <i aria-hidden="true" className={cn("pk-i", `pk-i--side-${side === "left" ? "l" : "r"}${shown ? "" : "-off"}`)} />
    </Btn>
  );
};

/** 顶栏只放：品牌、收起左栏、项目名、额度与花费、设置、收起右栏（「谁来干」在左栏 AGENTS 里选）。 */
export const TopBar: React.FC<{
  native: boolean;
  project: Workspace | null;
  page: "" | "settings" | "quota";
  onPage: (page: "settings" | "quota") => void;
  left: boolean;
  right: boolean;
  onLeft: () => void;
  onRight: () => void;
}> = ({ native, project, page, onPage, left, right, onLeft, onRight }) => (
  <header className={cn("app-top", native && "app-top--native")} data-tauri-drag-region>
    <div className="brand" data-tauri-drag-region>
      <b>PECK</b>
      <i className="brand-mark" />
    </div>
    <SideBtn side="left" shown={left} onToggle={onLeft} />
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
    <Btn v="ghost" className={cn(page === "quota" && "is-here")} aria-label="额度与花费" aria-pressed={page === "quota"} onClick={() => onPage("quota")}>QUOTA</Btn>
    <Btn v="ghost" square className={cn("top-gear", page === "settings" && "is-here")} aria-label="设置" aria-pressed={page === "settings"} onClick={() => onPage("settings")}>
      <Icon name="gear" />
    </Btn>
    <SideBtn side="right" shown={right} onToggle={onRight} />
  </header>
);
