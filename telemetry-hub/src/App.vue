<script setup>
import { ref, onMounted, computed } from "vue";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

const truck = ref({ speed: 0, limit: 0, gear: 0, fuel: 0, temp: 0, damage: 0 });

// --- 🧠 LOGIC ---
const isSpeeding = computed(() => truck.value.limit > 0 && truck.value.speed > truck.value.limit + 2);
const isLowFuel = computed(() => truck.value.fuel < 15);

const gearDisplay = computed(() => {
  if (truck.value.gear === 0) return 'N';
  if (truck.value.gear < 0) return 'R';
  return truck.value.gear.toString();
});

onMounted(async () => {
  // 🚀 Start listening for the data loop from Rust lib.rs
  try {
    await listen("telemetry-update", (event) => {
      truck.value = event.payload;
    });

    /** * 🚨 PRO FIX: 
     * We REMOVE setIgnoreCursorEvents(true) from here.
     * We will handle click-through via CSS 'pointer-events: none'.
     * This is much more reliable for dragging.
     */
  } catch (err) {
    console.error("HUD Bridge Error:", err);
  }
});
</script>

<template>
  <div class="hud-container">
    
    <div class="drag-handle" data-tauri-drag-region>✥</div>

    <div class="ribbon-content">
      <div class="group-left">
        <div class="stat-group">
          <span class="label">DAMAGE</span>
          <span class="value md" :class="{ 'warn': truck.damage > 5 }">{{ truck.damage }}%</span>
        </div>
        <div class="stat-group">
          <span class="label">TEMP</span>
          <span class="value md">{{ Math.round(truck.temp) }}°C</span>
        </div>
      </div>

      <div class="group-center-integrated">
        <div class="stat-group speed-box">
          <span class="label">SPEED</span>
          <div class="value-row">
            <span class="value lg" :class="{ 'speeding': isSpeeding }">{{ Math.round(truck.speed) }}</span>
            <div class="limit-sign-lg" v-if="truck.limit > 0">{{ Math.round(truck.limit) }}</div>
          </div>
        </div>

        <div class="stat-group gear-box">
          <span class="label">GEAR</span>
          <span class="value lg accent">{{ gearDisplay }}</span>
        </div>

        <div class="stat-group fuel-integrated">
          <span class="label">FUEL</span>
          <div class="fuel-track-lg">
            <div 
              class="fuel-fill" 
              :class="{ 'low-fuel-blink': isLowFuel }" 
              :style="{ width: truck.fuel + '%' }"
            ></div>
          </div>
        </div>

        <div class="stat-group branding-integrated">
          <span class="label">SIMNATION</span>
          <span class="status-text-lg">HUB ACTIVE</span>
        </div>
      </div>

      <div class="group-right-spacer"></div>
    </div>
  </div>
</template>

<style>
:root { background-color: transparent !important; user-select: none; }
body { margin: 0; overflow: hidden; background-color: transparent !important; }

.hud-container {
  width: 100%;
  height: 100px;
  background: linear-gradient(to bottom, rgba(5, 7, 12, 0.95) 0%, rgba(5, 7, 12, 0) 100%);
  border-top: 4px solid #FF5722;
  border-bottom-left-radius: 20px;
  border-bottom-right-radius: 20px;
  display: flex;
  align-items: center;
  padding: 0 50px;
  color: white;
  font-family: 'Inter', sans-serif;
  position: relative;
  
  /* 🚨 CLICK-THROUGH MAGIC:
     This makes the entire bar transparent to clicks so you can click the game.
  */
  pointer-events: none !important;
}

.drag-handle {
  position: absolute;
  left: 15px;
  top: 15px;
  color: #FF5722;
  opacity: 0.3;
  font-size: 24px;
  
  /* 🚨 INTERACTIVE HANDLE:
     This RE-ENABLES pointer events for the icon only.
     Windows will see this element as 'Solid' while the rest is 'Hollow'.
  */
  pointer-events: auto !important;
  -webkit-app-region: drag !important;
  cursor: grab;
  z-index: 9999;
}

.drag-handle:hover {
  opacity: 1;
  transform: scale(1.1);
}

.drag-handle:active {
  cursor: grabbing;
}

.ribbon-content { 
  display: flex; 
  width: 100%; 
  justify-content: space-between; 
  align-items: center; 
  /* Content is visual only, mouse passes through */
  pointer-events: none; 
}

/* --- Keep all your layout styles below exactly as they were --- */
.group-left { display: flex; gap: 40px; align-items: center; min-width: 280px; }
.group-right-spacer { min-width: 280px; }
.group-center-integrated { display: flex; gap: 70px; align-items: center; justify-content: center; flex-grow: 1; }
.stat-group { display: flex; flex-direction: column; }
.label { font-size: 10px; font-weight: 900; color: #999; letter-spacing: 3px; margin-bottom: 2px; }
.value.lg { font-size: 52px; font-weight: 950; line-height: 0.9; }
.value.md { font-size: 28px; font-weight: 900; margin-top: 4px; }
.accent { color: #FF5722; }
.warn { color: #ff3d00; text-shadow: 0 0 15px rgba(255, 61, 0, 0.6); }
.fuel-integrated { margin-top: 8px; }
.fuel-track-lg { width: 120px; height: 12px; background: rgba(255, 255, 255, 0.15); border-radius: 6px; overflow: hidden; border: 1px solid rgba(255,255,255,0.2); }
.fuel-fill { height: 100%; background: linear-gradient(90deg, #FF5722, #ff8a65); transition: width 0.5s ease-out; }
.branding-integrated { opacity: 0.6; border-left: 2px solid rgba(255,255,255,0.1); padding-left: 30px; }
.status-text-lg { font-size: 14px; font-weight: 900; color: #FF5722; margin-top: 6px; letter-spacing: 1px; }
.value-row { display: flex; align-items: center; gap: 15px; }
.limit-sign-lg { width: 42px; height: 42px; background: white; border: 4px solid #cc0000; border-radius: 50%; color: black; font-weight: 950; font-size: 18px; display: flex; justify-content: center; align-items: center; }
.speeding { color: #ff3d00; text-shadow: 0 0 25px rgba(255, 61, 0, 1); animation: speed-pulse 0.8s infinite; }
@keyframes speed-pulse { 0% { transform: scale(1); } 50% { transform: scale(1.1); } 100% { transform: scale(1); } }
.low-fuel-blink { background: #ff0000 !important; animation: fuel-pulse 1s infinite; }
@keyframes fuel-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }
</style>