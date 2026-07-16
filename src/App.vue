<script setup lang="ts">
/// App shell：64px 图标竖栏 + 内容区。
///
/// 竖栏固定 64px、永久折叠（无展开机制），菜单 4 个图标入口，
/// 设置入口搬到底部和主题切换并排。「顶部品牌名」「底部版本号」
/// 都已删除——图标自身就是入口，文字和元数据在竖栏里是噪音。

import { computed, h, onMounted } from "vue"
import {
  NConfigProvider,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NMessageProvider,
  NIcon,
  NTooltip,
  darkTheme,
  lightTheme,
} from "naive-ui"
import {
  Library,
  Inbox,
  Recycle,
  AlertTriangle,
  Settings as SettingsIcon,
  Sun,
  Moon,
  SunMoon,
} from "@lucide/vue"
import { RouterView, useRoute, useRouter } from "vue-router"
import { useThemeStore, useScanStatusStore } from "@/stores"
import ScanProgressToast from "@/components/ScanProgressToast.vue"
import { buildThemeOverrides } from "./styles/theme-overrides"
import hljs from "./lib/hljs"

const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()
const scanStatusStore = useScanStatusStore()

/// 启动时订阅 scanner-status 事件 + 拉一次快照。
onMounted(() => {
  scanStatusStore.init()
})

const activeKey = computed(() => route.name as string)

/// NMenu 的 icon 字段接受 render 函数 / 组件。包一层 NIcon 让 size
/// 走 n-menu 的 collapsed-icon-size，stroke-width 走自定义保持视觉一致。
function siderIcon(Icon: typeof Library) {
  return () => h(NIcon, null, () => h(Icon, { "stroke-width": 1.6 }))
}

const menuOptions = [
  { label: "我的同人志", key: "library", icon: siderIcon(Library) },
  { label: "入库冲突处理", key: "inbox", icon: siderIcon(Inbox) },
  { label: "文件回收站", key: "recycle", icon: siderIcon(Recycle) },
  { label: "脏数据", key: "dirty", icon: siderIcon(AlertTriangle) },
]

function handleMenu(key: string) {
  router.push({ name: key })
}

function goSettings() {
  router.push({ name: "settings" })
}

const themeOverrides = computed(() => buildThemeOverrides(themeStore.isDark))
const naiveTheme = computed(() => (themeStore.isDark ? darkTheme : lightTheme))

/// sider 上的主题按钮图标：system → SunMoon（半日半月），否则按当前生效显示。
const themeIcon = computed(() => {
  if (themeStore.mode === "system") return SunMoon
  return themeStore.isDark ? Moon : Sun
})

/// 单按钮循环切换：system → light → dark → system。
const NEXT_MODE: Record<string, "system" | "light" | "dark"> = {
  system: "light",
  light: "dark",
  dark: "system",
}
function cycleTheme() {
  themeStore.setMode(NEXT_MODE[themeStore.mode] ?? "system")
}
</script>

<template>
  <n-config-provider :theme="naiveTheme" :theme-overrides="themeOverrides" :hljs="hljs">
    <n-message-provider>
      <n-layout class="app-shell" has-sider position="absolute">
        <n-layout-sider
          class="app-sider"
          bordered
          :width="64"
          :collapsed-width="64"
          :collapsed="true"
          :native-scrollbar="false"
          :show-trigger="false"
        >
          <n-menu
            class="app-menu"
            :value="activeKey"
            :options="menuOptions"
            :collapsed="true"
            :collapsed-width="64"
            :collapsed-icon-size="22"
            :indent="18"
            @update:value="handleMenu"
          />

          <div class="sider-bottom">
            <n-tooltip placement="right">
              <template #trigger>
                <button
                  class="sider-icon-btn"
                  :aria-label="`主题：${themeStore.mode === 'system' ? '随系统' : themeStore.mode === 'light' ? '浅色' : '深色'}`"
                  @click="cycleTheme"
                >
                  <component
                    :is="themeIcon"
                    :size="22"
                    :stroke-width="1.6"
                  />
                </button>
              </template>
              {{
                themeStore.mode === "system" ? "随系统"
                : themeStore.mode === "light" ? "浅色"
                : "深色"
              }}
            </n-tooltip>

            <n-tooltip placement="right">
              <template #trigger>
                <button
                  class="sider-icon-btn"
                  aria-label="设置"
                  :class="{ 'is-active': activeKey === 'settings' }"
                  @click="goSettings"
                >
                  <component :is="SettingsIcon" :size="22" :stroke-width="1.6" />
                </button>
              </template>
              设置
            </n-tooltip>
          </div>
        </n-layout-sider>

        <n-layout-content class="app-content">
          <router-view />
          <ScanProgressToast />
        </n-layout-content>
      </n-layout>
    </n-message-provider>
  </n-config-provider>
</template>

<style scoped>
.app-shell {
  background: var(--surface-canvas);
}
.app-sider {
  background: var(--surface-canvas) !important;
  border-right: 1px solid var(--surface-border) !important;
}
.app-sider :deep(.n-menu-item-content) {
  justify-content: center;
}
.app-sider :deep(.n-menu-item-content__icon) {
  margin-right: 0;
}

.app-menu {
  padding: 8px 0;
}

/* 底部按钮绝对定位贴底：n-layout-sider-scroll-container 子项默认
   stack 自然流，绝对定位脱离正常流避免菜单多时被挤变形。 */
.sider-bottom {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 12px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  pointer-events: none;
}
.sider-bottom > * {
  pointer-events: auto;
}

.sider-icon-btn {
  width: 40px;
  height: 40px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 8px;
  color: var(--color-silver-mist);
  cursor: pointer;
  transition: color 0.15s, background-color 0.15s, border-color 0.15s;
}
.sider-icon-btn:hover {
  color: var(--color-snow);
  background: var(--color-ash);
  border-color: var(--surface-border);
}
.sider-icon-btn:active {
  background: var(--color-charcoal);
}
.sider-icon-btn.is-active {
  color: var(--color-phosphor-green);
  background: var(--color-forest-depth);
}

.sider-icon {
  width: 22px;
  height: 22px;
}

.app-content {
  background: var(--surface-canvas);
  width: 100%;
  height: 100%;
  overflow-y: auto;
  transition: background-color 0.2s ease;
}
</style>
