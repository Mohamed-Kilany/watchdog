// Read-only face-box overlay for the live viewfinder (M6-T04).
//
// The app reports `faces` as normalized boxes; drawing them in the DOM keeps
// them out of the JPEG stream and lets them track the browser's own scaling.

import { attachStyles, baseCss, defineOnce } from "./base";

/** One face rectangle reported by the app (`faces` control). */
export interface FaceBox {
  id?: number;
  score?: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

const css = `${baseCss}
  :host {
    position: absolute;
    inset: 0;
    display: block;
    pointer-events: none;
  }
  :host([hidden]) {
    display: none;
  }
  .box {
    position: absolute;
    border: 2px solid #66e3a4;
    border-radius: 0.35rem;
    background: rgb(102 227 164 / 8%);
  }
  .tag {
    position: absolute;
    left: -2px;
    bottom: 100%;
    margin-bottom: 0.15rem;
    padding: 0.05rem 0.3rem;
    border-radius: 0.25rem;
    background: rgb(5 8 11 / 72%);
    color: #66e3a4;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.62rem;
    font-weight: 700;
    white-space: nowrap;
  }
`;

function validFace(value: unknown): FaceBox | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const box: FaceBox = {
    x: Number(record["x"]),
    y: Number(record["y"]),
    w: Number(record["w"]),
    h: Number(record["h"]),
  };
  if (![box.x, box.y, box.w, box.h].every(Number.isFinite)) return null;
  if (typeof record["id"] === "number") box.id = record["id"];
  if (typeof record["score"] === "number") box.score = record["score"];
  return box;
}

export class WdFaceOverlay extends HTMLElement {
  private _faces: FaceBox[] = [];
  private rendered = false;
  private boxes!: HTMLDivElement;

  get faces(): FaceBox[] {
    return this._faces;
  }

  set faces(value: FaceBox[]) {
    const list = Array.isArray(value) ? value : [];
    this._faces = list
      .map(validFace)
      .filter((box): box is FaceBox => box !== null);
    this.render();
  }

  connectedCallback(): void {
    if (this.rendered) return;
    const root = this.attachShadow({ mode: "open" });
    attachStyles(root, css);
    root.innerHTML = `<div class="boxes"></div>`;
    this.boxes = root.querySelector(".boxes") as HTMLDivElement;
    this.rendered = true;
    this.render();
  }

  private render(): void {
    if (!this.rendered) return;
    this.boxes.replaceChildren();
    this.hidden = this._faces.length === 0;
    for (const [index, face] of this._faces.entries()) {
      const box = document.createElement("div");
      box.className = "box";
      box.style.left = `${face.x * 100}%`;
      box.style.top = `${face.y * 100}%`;
      box.style.width = `${face.w * 100}%`;
      box.style.height = `${face.h * 100}%`;
      const tag = document.createElement("span");
      tag.className = "tag";
      const number = (face.id ?? index) + 1;
      tag.textContent =
        face.score === undefined
          ? `face ${number}`
          : `face ${number} · ${Math.round(face.score)}%`;
      box.append(tag);
      this.boxes.append(box);
    }
  }
}

defineOnce("wd-face-overlay", WdFaceOverlay);
