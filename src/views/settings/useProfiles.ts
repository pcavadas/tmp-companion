// src/views/settings/useProfiles.ts — the instrument-profile list operations (add /
// edit / reorder / delete) plus the pane's add-form and edit-row state. Each persists
// the whole list through the store's optimistic `persistProfiles`.

import { useState } from "react";

import type { Profile } from "../../lib/types";
import type { SettingsStore } from "./useSettingsStore";

export interface ProfileDraft {
  name: string;
  topology_id: string;
}

export interface ProfileOps {
  /** The "Add instrument" form is open. */
  adding: boolean;
  setAdding: (open: boolean) => void;
  /** The profile whose row is swapped for the edit form. */
  editingId: string | null;
  setEditingId: (id: string | null) => void;
  addInstrument: (data: ProfileDraft) => void;
  saveEdit: (id: string, data: ProfileDraft) => void;
  deleteProfile: (id: string) => void;
  moveProfile: (id: string, dir: -1 | 1) => void;
}

export function useProfiles({
  profiles,
  persistProfiles,
}: Pick<SettingsStore, "profiles" | "persistProfiles">): ProfileOps {
  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);

  function addInstrument(data: ProfileDraft) {
    const id = crypto.randomUUID();
    const next: Profile[] = [
      ...profiles,
      {
        id,
        name: data.name,
        topology_id: data.topology_id,
        calibration_lufs: null,
      },
    ];
    void persistProfiles(next);
    setAdding(false);
  }

  function saveEdit(id: string, data: ProfileDraft) {
    void persistProfiles(
      profiles.map((p) =>
        p.id === id
          ? { ...p, name: data.name, topology_id: data.topology_id }
          : p,
      ),
    );
    setEditingId(null);
  }

  function deleteProfile(id: string) {
    void persistProfiles(profiles.filter((p) => p.id !== id));
  }

  function moveProfile(id: string, dir: -1 | 1) {
    const i = profiles.findIndex((p) => p.id === id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= profiles.length) return;
    const next = [...profiles];
    [next[i], next[j]] = [next[j], next[i]];
    void persistProfiles(next);
  }

  return {
    adding,
    setAdding,
    editingId,
    setEditingId,
    addInstrument,
    saveEdit,
    deleteProfile,
    moveProfile,
  };
}
