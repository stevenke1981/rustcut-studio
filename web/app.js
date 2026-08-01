const $ = (selector) => document.querySelector(selector);
const state = { project: null, projects: [], selectedAsset: null };

const api = async (path, options = {}) => {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json", ...(options.headers || {}) },
    ...options,
  });
  if (response.status === 204) return null;
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(payload.error || `${response.status} ${response.statusText}`);
  return payload;
};

const setStatus = (message) => { $("#status").textContent = message; };
const formatDuration = (ms = 0) => `${(ms / 1000).toFixed(1)}s`;
const escapeHtml = (value = "") => value.replace(/[&<>'"]/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;","'":"&#39;",'"':"&quot;"}[c]));

async function loadProjects(selectNewest = false) {
  const payload = await api("/api/projects");
  state.projects = payload.projects || [];
  const select = $("#projectSelect");
  select.innerHTML = state.projects.map(project => `<option value="${project.id}">${escapeHtml(project.name)}</option>`).join("");
  if (!state.projects.length) {
    state.project = null;
    renderProject();
    return;
  }
  const target = selectNewest ? state.projects[0].id : (state.project?.id || state.projects[0].id);
  select.value = target;
  await loadProject(target);
}

async function loadProject(id) {
  setStatus("載入專案…");
  state.project = await api(`/api/projects/${id}`);
  $("#projectSelect").value = id;
  renderProject();
  setStatus("就緒");
}

function renderProject() {
  const project = state.project;
  if (!project) {
    $("#previewMeta").textContent = "選取素材後開始編輯";
    $("#assetList").className = "asset-list empty-state";
    $("#assetList").textContent = "請先新增專案";
    $("#timeline").className = "timeline empty-state";
    $("#timeline").textContent = "時間軸尚無片段";
    return;
  }
  const assets = Object.values(project.assets || {});
  const clipCount = project.timeline.tracks.reduce((total, track) => total + track.clips.length, 0);
  $("#previewMeta").textContent = `${assets.length} 個素材 · ${clipCount} 個時間軸片段`;
  $("#assetCount").textContent = assets.length;
  $("#projectMeta").textContent = `${project.timeline.settings.width}×${project.timeline.settings.height} · ${project.timeline.settings.fps} fps · rev ${project.revision}`;
  renderAssets(assets);
  renderTimeline(project.timeline);
  renderEffectAssetOptions(assets);
}

function renderAssets(assets) {
  const list = $("#assetList");
  if (!assets.length) {
    list.className = "asset-list empty-state";
    list.textContent = "尚未匯入素材";
    return;
  }
  list.className = "asset-list";
  list.innerHTML = assets.map(asset => `
    <article class="asset-card">
      <strong title="${escapeHtml(asset.name)}">${escapeHtml(asset.name)}</strong>
      <small>${asset.kind} · ${formatDuration(asset.duration_ms)} ${state.project.transcripts?.[asset.id] ? "· 已有逐字稿" : ""}</small>
      <div class="asset-actions">
        <button data-preview="${asset.id}">預覽</button>
        <button data-add="${asset.id}">加入時間軸</button>
      </div>
      <div class="asset-actions"><button data-transcript="${asset.id}">匯入字幕／逐字稿</button></div>
    </article>`).join("");
  list.querySelectorAll("[data-preview]").forEach(button => button.onclick = () => previewAsset(button.dataset.preview));
  list.querySelectorAll("[data-add]").forEach(button => button.onclick = () => addAsset(button.dataset.add));
  list.querySelectorAll("[data-transcript]").forEach(button => button.onclick = () => importTranscript(button.dataset.transcript));
}

function previewAsset(assetId) {
  state.selectedAsset = assetId;
  const viewer = $("#viewer");
  viewer.src = `/api/projects/${state.project.id}/assets/${assetId}/file`;
  $("#viewerEmpty").style.display = "none";
  viewer.play().catch(() => {});
}

function renderTimeline(timeline) {
  const root = $("#timeline");
  const duration = Math.max(timeline.tracks.flatMap(t => t.clips).reduce((max, clip) => Math.max(max, clip.start_ms + Math.max(0, clip.source_out_ms - clip.source_in_ms) / Math.max(.01, clip.speed)), 0), 1000);
  const tracks = timeline.tracks.filter(track => track.clips.length || ["video","audio","caption","graphic"].includes(track.kind));
  if (!tracks.some(track => track.clips.length)) {
    root.className = "timeline empty-state";
    root.textContent = "時間軸尚無片段";
    return;
  }
  root.className = "timeline";
  root.innerHTML = tracks.map(track => `
    <div class="track-row track-${track.kind}">
      <div class="track-name">${escapeHtml(track.name)}<br><small>${track.kind}</small></div>
      <div class="track-lane">${track.clips.map(clip => {
        const start = clip.start_ms / duration * 100;
        const width = ((clip.source_out_ms - clip.source_in_ms) / Math.max(.01, clip.speed)) / duration * 100;
        const label = clip.text?.text || clip.name;
        const animation = clip.text?.animation && clip.text.animation !== "none" ? ` · ${clip.text.animation}` : "";
        return `<div class="clip" style="left:${start}%;width:${Math.max(width, .6)}%" title="${escapeHtml(label + animation)}">${escapeHtml(label)}${animation ? `<small>${escapeHtml(animation)}</small>` : ""}</div>`;
      }).join("")}</div>
    </div>`).join("");
}

function renderEffectAssetOptions(assets) {
  const select = $("#captionAsset");
  const eligible = assets.filter(asset => state.project?.transcripts?.[asset.id]);
  select.innerHTML = eligible.length
    ? eligible.map(asset => `<option value="${asset.id}">${escapeHtml(asset.name)}</option>`).join("")
    : '<option value="">請先匯入逐字稿</option>';
}

function textStyle({
  text = "",
  fontSize = 64,
  color = "#ffffff",
  position = "bottom",
  animation = "none",
  boxColor = null,
  outline = false,
  boxed = false,
  shadow = false,
  preset = null,
} = {}) {
  if (preset === "outline") {
    outline = true;
  }
  if (preset === "boxed") {
    boxed = true;
  }
  if (preset === "shadow") {
    shadow = true;
  }
  return {
    text,
    font_file: null,
    font_size: Number(fontSize),
    color,
    opacity: 1,
    outline_color: "#000000",
    outline_width: outline ? 4 : 2,
    box_color: boxed ? `${boxColor}cc` : null,
    box_padding: 20,
    shadow_color: shadow ? "#000000cc" : null,
    shadow_x: 4,
    shadow_y: 4,
    position,
    alignment: "center",
    animation,
    animation_duration_ms: 420,
  };
}

async function applyCommands(commands, successMessage) {
  if (!state.project) throw new Error("請先建立或選擇專案");
  setStatus("套用特效…");
  state.project = await api(`/api/projects/${state.project.id}/apply`, {
    method: "POST",
    body: JSON.stringify({ commands }),
  });
  renderProject();
  setStatus(successMessage);
  addMessage(successMessage, "assistant");
}

function timelineDurationMs() {
  if (!state.project) return 0;
  return state.project.timeline.tracks
    .flatMap(track => track.clips)
    .reduce(
      (max, clip) =>
        Math.max(
          max,
          clip.start_ms + Math.max(0, clip.source_out_ms - clip.source_in_ms) / Math.max(.01, clip.speed),
        ),
      0
    );
}

function updateEffectPreview() {
  const preview = $("#effectPreview");
  const text = $("#cardText").value.trim();
  if (!text) {
    preview.textContent = "";
    preview.className = "effect-preview";
    return;
  }
  preview.textContent = text;
  preview.style.color = $("#cardColor").value;
  preview.style.background = $("#cardBoxEnabled").checked ? `${$("#cardBoxColor").value}cc` : "transparent";
  preview.dataset.position = $("#cardPosition").value;
  preview.className = `effect-preview animation-${$("#cardAnimation").value}`;
  preview.getAnimations().forEach(animation => animation.cancel());
  void preview.offsetWidth;
  preview.classList.add("is-playing");
}

async function addAsset(assetId) {
  try {
    setStatus("加入時間軸…");
    state.project = await api(`/api/projects/${state.project.id}/timeline/add`, { method: "POST", body: JSON.stringify({ asset_id: assetId }) });
    renderProject();
    setStatus("素材已加入時間軸");
  } catch (error) { showError(error); }
}

async function importTranscript(assetId) {
  const path = prompt("輸入 JSON、SRT 或 VTT 檔案的本機完整路徑：");
  if (!path) return;
  try {
    state.project = await api(`/api/projects/${state.project.id}/transcripts/import`, { method: "POST", body: JSON.stringify({ asset_id: assetId, path }) });
    renderProject();
    setStatus("逐字稿已匯入");
  } catch (error) { showError(error); }
}

async function sendPrompt(autoApply) {
  const promptText = $("#promptInput").value.trim();
  if (!promptText || !state.project) return;
  addMessage(promptText, "user");
  $("#promptInput").value = "";
  setStatus(autoApply ? "套用剪輯…" : "產生計畫…");
  try {
    const payload = await api(`/api/projects/${state.project.id}/${autoApply ? "prompt" : "plan"}`, {
      method: "POST",
      body: JSON.stringify({ prompt: promptText, planner: $("#plannerSelect").value, auto_apply: autoApply }),
    });
    const plan = autoApply ? payload.plan : payload;
    addMessage(`${plan.summary}\n\n${plan.commands.map(command => `• ${command.type}`).join("\n") || "沒有命令"}${plan.warnings.length ? `\n\n注意：${plan.warnings.join("；")}` : ""}`, "assistant");
    if (payload.project) state.project = payload.project;
    else state.project = await api(`/api/projects/${state.project.id}`);
    renderProject();
    setStatus("就緒");
  } catch (error) { showError(error); }
}

function addMessage(text, kind) {
  const article = document.createElement("article");
  article.className = `message ${kind}`;
  article.textContent = text;
  $("#chatLog").appendChild(article);
  $("#chatLog").scrollTop = $("#chatLog").scrollHeight;
}

function showError(error) {
  console.error(error);
  setStatus("發生錯誤");
  addMessage(error.message || String(error), "error");
}

$("#newProjectBtn").onclick = () => $("#newProjectDialog").showModal();
$("#newProjectForm").addEventListener("submit", async event => {
  event.preventDefault();
  const [width, height] = $("#newProjectPreset").value.split("x").map(Number);
  try {
    await api("/api/projects", { method: "POST", body: JSON.stringify({ name: $("#newProjectName").value, width, height, fps: 30 }) });
    $("#newProjectDialog").close();
    await loadProjects(true);
  } catch (error) { showError(error); }
});
$("#projectSelect").onchange = event => loadProject(event.target.value).catch(showError);
$("#importForm").addEventListener("submit", async event => {
  event.preventDefault();
  if (!state.project) return;
  try {
    setStatus("分析與複製素材…");
    state.project = await api(`/api/projects/${state.project.id}/assets/import`, { method: "POST", body: JSON.stringify({ path: $("#mediaPath").value, copy: true }) });
    $("#mediaPath").value = "";
    renderProject();
    setStatus("素材已匯入");
  } catch (error) { showError(error); }
});
$("#chatForm").addEventListener("submit", event => { event.preventDefault(); sendPrompt(true); });
$("#planOnlyBtn").onclick = () => sendPrompt(false);
$("#undoBtn").onclick = async () => { if (!state.project) return; try { state.project = await api(`/api/projects/${state.project.id}/undo`, {method:"POST", body:"{}"}); renderProject(); } catch (e) { showError(e); } };
$("#redoBtn").onclick = async () => { if (!state.project) return; try { state.project = await api(`/api/projects/${state.project.id}/redo`, {method:"POST", body:"{}"}); renderProject(); } catch (e) { showError(e); } };
$("#renderBtn").onclick = async () => {
  if (!state.project) return;
  try {
    setStatus("排入輸出工作…");
    const job = await api(`/api/projects/${state.project.id}/render`, { method: "POST", body: "{}" });
    addMessage(`已開始輸出：${job.output}`, "assistant");
    pollJob(job.id);
  } catch (error) { showError(error); }
};

$("#captionForm").addEventListener("submit", async event => {
  event.preventDefault();
  const assetId = $("#captionAsset").value;
  if (!assetId) return showError(new Error("請先為素材匯入字幕或逐字稿"));
  const preset = $("#captionPreset").value;
  try {
    await applyCommands([{
      type: "add_captions",
      asset_id: assetId,
      preset,
      style: textStyle({
        fontSize: Number($("#captionFontSize").value),
        position: "bottom",
        boxed: true,
        boxColor: "#000000",
      }),
    }], "字幕軌已產生");
  } catch (error) { showError(error); }
});

$("#textCardForm").addEventListener("submit", async event => {
  event.preventDefault();
  const startMs = Math.round(Number($("#cardStart").value) * 1000);
  const endMs = Math.round(Number($("#cardEnd").value) * 1000);
  try {
    await applyCommands([{
      type: "add_text",
      text: textStyle({
        text: $("#cardText").value.trim(),
        fontSize: 72,
        color: $("#cardColor").value,
        position: $("#cardPosition").value,
        animation: $("#cardAnimation").value,
        boxColor: $("#cardBoxEnabled").checked ? $("#cardBoxColor").value : null,
      }),
      start_ms: startMs,
      end_ms: endMs,
    }], "字卡已加入時間軸");
  } catch (error) { showError(error); }
});

$("#borderForm").addEventListener("submit", async event => {
  event.preventDefault();
  try {
    await applyCommands([{
      type: "add_border",
      color: $("#borderColor").value,
      width: Number($("#borderWidth").value),
    }], "全片邊框已套用");
  } catch (error) { showError(error); }
});

$("#watermarkForm").addEventListener("submit", async event => {
  event.preventDefault();
  const duration = Math.round(timelineDurationMs());
  try {
    await applyCommands([{
      type: "add_watermark",
      text: $("#watermarkText").value.trim(),
      font_file: null,
      position: $("#watermarkPosition").value,
      font_size: 32,
      color: "#ffffff",
      opacity: Number($("#watermarkOpacity").value),
      start_ms: 0,
      end_ms: duration || null,
    }], "全片浮水印已加入");
  } catch (error) { showError(error); }
});

["#cardText", "#cardPosition", "#cardAnimation", "#cardColor", "#cardBoxColor", "#cardBoxEnabled"]
  .forEach(selector => $(selector).addEventListener("input", updateEffectPreview));
updateEffectPreview();

async function pollJob(jobId) {
  try {
    const job = await api(`/api/jobs/${jobId}`);
    setStatus(`輸出：${job.status}`);
    if (["queued","running"].includes(job.status)) setTimeout(() => pollJob(jobId), 1200);
    else if (job.status === "completed") addMessage(`輸出完成：${job.output}`, "assistant");
    else showError(new Error(job.error || "輸出失敗"));
  } catch (error) { showError(error); }
}

loadProjects().catch(showError);
