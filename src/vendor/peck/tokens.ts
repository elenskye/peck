// 生成文件，请勿手改。源：schemes/peck/tokens/
export const tokens = {
  "scheme": "peck",
  "color": {
    "page": {
      "light": "#F2F4FC",
      "dark": "#050816"
    },
    "grid": {
      "light": "rgba(40,50,130,.10)",
      "dark": "rgba(120,140,255,.085)"
    },
    "panel": {
      "light": "#FFFFFF",
      "dark": "#0A0F24"
    },
    "panel-2": {
      "light": "#F5F7FD",
      "dark": "#0F1630"
    },
    "raised": {
      "light": "#E7EBF8",
      "dark": "#172040"
    },
    "line": {
      "light": "#DAE0F0",
      "dark": "#1E2850"
    },
    "line-strong": {
      "light": "#B4BDDB",
      "dark": "#43528B"
    },
    "block-off": {
      "light": "#E1E6F5",
      "dark": "#141C3C"
    },
    "text": {
      "light": "#0B1030",
      "dark": "#E8ECFF"
    },
    "text-2": {
      "light": "#4E567A",
      "dark": "#BEC7F5"
    },
    "text-3": {
      "light": "#646C92",
      "dark": "#9FAADE"
    },
    "cyan": {
      "light": "#007A94",
      "dark": "#00E5FF"
    },
    "cyan-hover": {
      "light": "#00687E",
      "dark": "#4DEDFF"
    },
    "on-cyan": {
      "light": "#FFFFFF",
      "dark": "#02101A"
    },
    "cyan-text": {
      "light": "#007A94",
      "dark": "#00E5FF"
    },
    "cyan-block": {
      "light": "#0099B8",
      "dark": "#00E5FF"
    },
    "cyan-line": {
      "light": "#0099B8",
      "dark": "#048FA7"
    },
    "cyan-soft": {
      "light": "#E2F4F8",
      "dark": "#09243A"
    },
    "cyan-scan": {
      "light": "rgba(0,153,184,.24)",
      "dark": "rgba(0,229,255,.22)"
    },
    "ok": {
      "light": "#12804C",
      "dark": "#4ADE80"
    },
    "ok-soft": {
      "light": "#E3F5EC",
      "dark": "#10242D"
    },
    "warn": {
      "light": "#A35E00",
      "dark": "#FFB86B"
    },
    "warn-hover": {
      "light": "#8B5000",
      "dark": "#FFCD97"
    },
    "warn-soft": {
      "light": "#FFF3E2",
      "dark": "#23202B"
    },
    "warn-line": {
      "light": "#A35E00",
      "dark": "#9D744F"
    },
    "on-warn": {
      "light": "#FFFFFF",
      "dark": "#02101A"
    },
    "bad": {
      "light": "#E11D48",
      "dark": "#FE9AA4"
    },
    "bad-hover": {
      "light": "#BF193D",
      "dark": "#FEB8BF"
    },
    "bad-soft": {
      "light": "#FFE9EE",
      "dark": "#221D31"
    },
    "bad-line": {
      "light": "#E11D48",
      "dark": "#A96977"
    },
    "on-bad": {
      "light": "#FFFFFF",
      "dark": "#050816"
    },
    "muse": {
      "light": "#6D4AE8",
      "dark": "#A78BFA"
    },
    "grok": {
      "light": "#A35E00",
      "dark": "#FFB86B"
    },
    "api": {
      "light": "#2563EB",
      "dark": "#60A5FA"
    }
  },
  "scale": {
    "space": {
      "1": "2px",
      "2": "4px",
      "3": "6px",
      "4": "8px",
      "5": "10px",
      "6": "12px",
      "7": "14px",
      "8": "16px",
      "9": "18px",
      "10": "22px",
      "11": "28px",
      "12": "30px"
    },
    "radius": {},
    "width": {
      "left": "288px",
      "right": "336px",
      "gap": "16px",
      "pad-top": "22px",
      "pad-x": "30px",
      "pad-bottom": "28px"
    },
    "text": {
      "hero": "24px",
      "title": "18px",
      "name": "12px",
      "body": "12.5px",
      "small": "12px",
      "code": "11px",
      "label": "13px",
      "button": "14px",
      "count": "17px",
      "num": "26px",
      "timer": "54px"
    },
    "font": {
      "name": "\"Fusion Pixel 12\",\"PingFang SC\",sans-serif",
      "body": "\"IBM Plex Sans\",\"PingFang SC\",\"Hiragino Sans GB\",sans-serif",
      "code": "\"Departure Mono\",\"Fusion Pixel 12\",\"PingFang SC\",monospace",
      "dots": "\"Doto\",\"IBM Plex Mono\",monospace"
    }
  },
  "motion": {
    "$note": "一格 50ms：一步一格（steps），位移一格一格跳，不用平滑缓动；时长都是整格。光（扫描、亮块呼吸）可以平滑，它不是东西。减弱动态效果时只留最后的样子。",
    "tickMs": 50,
    "tiers": {
      "calm": {
        "use": "控件的小动静：状态立刻变；出现、露出、溶掉 4–8 格。",
        "stateMs": [
          0,
          0
        ],
        "enterMs": 400,
        "enterEasing": "steps",
        "maxMsWithoutSignature": 400,
        "forbid": [
          "loop",
          "parallax",
          "smooth-translate",
          "page-transition"
        ]
      },
      "expressive": {
        "use": "发出、等你、做完、撤销这几个时刻，和在跑、等待时的循环；要登记。",
        "maxSingleMs": 1000,
        "registeredSurfacesOnly": true,
        "loopRules": "只有光标、点阵转圈、进度块、在跑那一步、在跑时的扫描能循环；减弱动态效果时停下",
        "registeredSurfaces": [
          "card-edge",
          "send",
          "scan-once",
          "done-flash",
          "wait-turn",
          "roll",
          "caret",
          "twinkle",
          "head-blink",
          "wave",
          "running",
          "scan-loop"
        ]
      }
    },
    "calm": {
      "state": {
        "label": "控件状态",
        "trigger": "悬停、按下、选中、开关",
        "ticks": 0,
        "ms": 0,
        "easing": "立刻变，不过渡"
      },
      "reveal-s": {
        "label": "小东西出现",
        "trigger": "提示条、标签、菜单",
        "ticks": 4,
        "ms": 200,
        "easing": "steps(4)"
      },
      "reveal": {
        "label": "露出",
        "trigger": "卡片内容、回答一段段出来",
        "ticks": 8,
        "ms": 400,
        "easing": "steps(8)"
      },
      "dissolve": {
        "label": "溶掉",
        "trigger": "撤销时改动一行行消失",
        "ticks": 8,
        "ms": 400,
        "easing": "steps(8)"
      }
    },
    "registeredSurfaces": {
      "card-edge": {
        "tier": "expressive",
        "label": "卡片的边点亮",
        "trigger": "停下来问你、高危卡片出现",
        "ticks": 16,
        "easing": "steps(16)：上 → 右 → 下 → 左",
        "maxMs": 800,
        "loop": false
      },
      "send": {
        "tier": "expressive",
        "label": "发出",
        "trigger": "发任务：一格光从输入框飞到任务",
        "ticks": 12,
        "easing": "steps(6)",
        "maxMs": 600,
        "loop": false
      },
      "scan-once": {
        "tier": "expressive",
        "label": "扫一次",
        "trigger": "发出、等你、完成、撤销",
        "ticks": 18,
        "easing": "平滑（光不是东西）",
        "maxMs": 900,
        "loop": false
      },
      "done-flash": {
        "tier": "expressive",
        "label": "做完闪两下",
        "trigger": "任务做完",
        "ticks": 18,
        "easing": "steps(1)",
        "maxMs": 900,
        "loop": false
      },
      "wait-turn": {
        "tier": "expressive",
        "label": "头一格变琥珀",
        "trigger": "停下来等你",
        "ticks": 20,
        "easing": "steps(1)",
        "maxMs": 1000,
        "loop": false
      },
      "roll": {
        "tier": "expressive",
        "label": "数字滚",
        "trigger": "撤销时文件数往下滚",
        "ticks": 18,
        "easing": "steps(n)，n 是滚过的数",
        "maxMs": 900,
        "loop": false
      },
      "caret": {
        "tier": "expressive",
        "label": "光标闪",
        "trigger": "输入框有焦点",
        "ticks": 20,
        "easing": "steps(1)",
        "loop": true,
        "loopMs": 1000
      },
      "twinkle": {
        "tier": "expressive",
        "label": "点阵转圈",
        "trigger": "短的等待（3×3 点阵）",
        "ticks": 24,
        "easing": "steps(1)",
        "loop": true,
        "loopMs": 1200
      },
      "head-blink": {
        "tier": "expressive",
        "label": "头一格闪",
        "trigger": "进度块正在走的那一格",
        "ticks": 16,
        "easing": "steps(1)",
        "loop": true,
        "loopMs": 800
      },
      "wave": {
        "tier": "expressive",
        "label": "亮块呼吸",
        "trigger": "进度块亮着的格",
        "ticks": 48,
        "easing": "缓入缓出（只变亮度）",
        "loop": true,
        "loopMs": 2400
      },
      "running": {
        "tier": "expressive",
        "label": "在跑那一步",
        "trigger": "弹扣一下 + 一圈光",
        "ticks": 28,
        "easing": "回弹 cubic-bezier(.3,1.6,.5,1)",
        "loop": true,
        "loopMs": 1400
      },
      "scan-loop": {
        "tier": "expressive",
        "label": "在跑时扫描",
        "trigger": "在跑的那一步每 2.8 秒扫一次",
        "ticks": 56,
        "easing": "平滑",
        "loop": true,
        "loopMs": 2800
      }
    },
    "calmExceptions": {}
  }
} as const;
export type ColorRole = keyof typeof tokens.color;
