import React, { useCallback, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";

interface SplitPaneProps {
  left: React.ReactNode;
  right: React.ReactNode;
  defaultLeftWidth?: number;
  minLeftWidth?: number;
  maxLeftWidth?: number;
}

export const SplitPane: React.FC<SplitPaneProps> = ({
  left,
  right,
  defaultLeftWidth = 420,
  minLeftWidth = 320,
  maxLeftWidth = 620,
}) => {
  const [leftWidth, setLeftWidth] = useState(defaultLeftWidth);
  const [isDragging, setIsDragging] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const nextWidth = Math.min(
        maxLeftWidth,
        Math.max(minLeftWidth, e.clientX - rect.left),
      );
      setLeftWidth(nextWidth);
    };

    const handleMouseUp = () => {
      setIsDragging(false);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isDragging, minLeftWidth, maxLeftWidth]);

  return (
    <div
      ref={containerRef}
      className="flex h-[calc(100vh-46px)] w-full overflow-hidden bg-[#0F1115]"
    >
      {/* Left Pane: ChatPanel */}
      <aside
        style={{ width: `${leftWidth}px` }}
        className="flex h-full shrink-0 flex-col bg-[#14171C]"
      >
        {left}
      </aside>

      {/* Resizable Divider */}
      <div
        role="separator"
        aria-orientation="vertical"
        onMouseDown={handleMouseDown}
        className={cn(
          "group relative z-20 flex h-full w-[5px] shrink-0 cursor-col-resize items-center justify-center transition-colors",
          isDragging ? "bg-[#00E5FF]/30" : "bg-transparent hover:bg-[#00E5FF]/20",
        )}
      >
        <div
          className={cn(
            "h-full w-[1px] transition-colors",
            isDragging
              ? "bg-[#00E5FF]"
              : "bg-[#262B34] group-hover:bg-[#00E5FF]",
          )}
        />
      </div>

      {/* Right Pane: ArtifactBoard */}
      <main className="flex h-full min-w-0 flex-1 flex-col bg-[#0F1115]">
        {right}
      </main>
    </div>
  );
};
