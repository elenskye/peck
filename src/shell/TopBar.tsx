import React from "react";
import type { Workspace } from "@/lib/types";
import { cn, shortenHomePath } from "@/lib/utils";
import { Btn, Icon } from "@/ui";

/** 顶栏只放：品牌、项目名、额度与花费、设置（「谁来干」在输入框下面的模型列表里选）。 */
export const TopBar: React.FC<{
  native: boolean;
  project: Workspace | null;
  page: "" | "settings" | "quota";
  onPage: (page: "settings" | "quota") => void;
}> = ({ native, project, page, onPage }) => (
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
    <Btn v="ghost" className={cn(page === "quota" && "is-here")} aria-label="额度与花费" aria-pressed={page === "quota"} onClick={() => onPage("quota")}>QUOTA</Btn>
    <Btn v="ghost" square className={cn("top-gear", page === "settings" && "is-here")} aria-label="设置" aria-pressed={page === "settings"} onClick={() => onPage("settings")}>
      <Icon name="gear" />
    </Btn>
  </header>
);
