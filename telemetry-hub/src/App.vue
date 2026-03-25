<script setup>
import { ref, onMounted, computed } from "vue";
import { listen } from "@tauri-apps/api/event";

// 🚀 Added the new data fields to the ref
const truck = ref({ 
  speed: 0, limit: 0, gear: 0, fuel: 0, temp: 0, damage: 0,
  rpm: 0, cruiseControl: 0, odometer: 0, routeDistance: 0, routeTime: 0
});

// --- 🧠 LOGIC ---
const isSpeeding = computed(() => truck.value.limit > 0 && truck.value.speed > truck.value.limit + 2);
const isLowFuel = computed(() => truck.value.fuel < 15);

const gearDisplay = computed(() => {
  if (truck.value.gear === 0) return 'N';
  if (truck.value.gear < 0) return 'R';
  return truck.value.gear.toString();
});

// 🗺️ Format GPS ETA into Hours and Minutes
const etaDisplay = computed(() => {
  if (truck.value.routeTime <= 0) return '--:--';
  const h = Math.floor(truck.value.routeTime / 60);
  const m = Math.floor(truck.value.routeTime % 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
});

onMounted(async () => {
  try {
    await listen("telemetry-update", (event) => {
      truck.value = event.payload;
    });
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
        <div class="stat-group">
          <span class="label">RPM</span>
          <span class="value md">{{ Math.round(truck.rpm) }}</span>
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

      <div class="group-right">
        <div class="stat-group">
          <span class="label">CRUISE</span>
          <span class="value md accent" v-if="truck.cruiseControl > 0">{{ Math.round(truck.cruiseControl) }}</span>
          <span class="value md" style="color: #555;" v-else>OFF</span>
        </div>
        
        <div class="stat-group" v-if="truck.routeDistance > 0">
          <span class="label">GPS DIST</span>
          <span class="value md">{{ Math.round(truck.routeDistance) }} km</span>
        </div>
        <div class="stat-group" v-if="truck.routeDistance > 0">
          <span class="label">ETA</span>
          <span class="value md">{{ etaDisplay }}</span>
        </div>
        
        <div class="stat-group" v-else>
          <span class="label">ODO</span>
          <span class="value md">{{ Math.round(truck.odometer).toLocaleString() }}</span>
        </div>
      </div>
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
  pointer-events: none !important;
}

.drag-handle {
  position: absolute;
  left: 15px;
  top: 15px;
  color: #FF5722;
  opacity: 0.3;
  font-size: 24px;
  pointer-events: auto !important;
  -webkit-app-region: drag !important;
  cursor: grab;
  z-index: 9999;
}

.drag-handle:hover { opacity: 1; transform: scale(1.1); }
.drag-handle:active { cursor: grabbing; }

.ribbon-content { 
  display: flex; 
  width: 100%; 
  justify-content: space-between; 
  align-items: center; 
  pointer-events: none; 
}

/* Layout Groups */
.group-left { display: flex; gap: 40px; align-items: center; min-width: 300px; }
.group-center-integrated { display: flex; gap: 70px; align-items: center; justify-content: center; flex-grow: 1; }
.group-right { display: flex; gap: 40px; align-items: center; min-width: 300px; justify-content: flex-end; }

/* Text & Visuals */
.stat-group { display: flex; flex-direction: column; }
.label { font-size: 10px; font-weight: 900; color: #999; letter-spacing: 3px; margin-bottom: 2px; }
.value.lg { font-size: 52px; font-weight: 950; line-height: 0.9; }
.value.md { font-size: 28px; font-weight: 900; margin-top: 4px; }
.accent { color: #FF5722; }
.warn { color: #ff3d00; text-shadow: 0 0 15px rgba(255, 61, 0, 0.6); }

/* Custom Elements */
.fuel-integrated { margin-top: 8px; }
.fuel-track-lg { width: 120px; height: 12px; background: rgba(255, 255, 255, 0.15); border-radius: 6px; overflow: hidden; border: 1px solid rgba(255,255,255,0.2); }
.fuel-fill { height: 100%; background: linear-gradient(90deg, #FF5722, #ff8a65); transition: width 0.5s ease-out; }
.branding-integrated { opacity: 0.6; border-left: 2px solid rgba(255,255,255,0.1); padding-left: 30px; }
.status-text-lg { font-size: 14px; font-weight: 900; color: #FF5722; margin-top: 6px; letter-spacing: 1px; }
.value-row { display: flex; align-items: center; gap: 15px; }
.limit-sign-lg { width: 42px; height: 42px; background: white; border: 4px solid #cc0000; border-radius: 50%; color: black; font-weight: 950; font-size: 18px; display: flex; justify-content: center; align-items: center; }

/* Animations */
.speeding { color: #ff3d00; text-shadow: 0 0 25px rgba(255, 61, 0, 1); animation: speed-pulse 0.8s infinite; }
@keyframes speed-pulse { 0% { transform: scale(1); } 50% { transform: scale(1.1); } 100% { transform: scale(1); } }
.low-fuel-blink { background: #ff0000 !important; animation: fuel-pulse 1s infinite; }
@keyframes fuel-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }
</style>