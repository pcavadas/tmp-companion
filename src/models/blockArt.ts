// Block-art catalog ported from the design handoff
// (illustration_engine/catalog-data.jsx): the authoritative per-model
// icon + chassis tone + terse label, keyed by firmware FenderId. This is
// what BlockArt needs to draw the real modeled unit — resolved BY ID (not
// by broad category), so e.g. Filtron gets the envelope-filter icon.

import type { IconId, ToneId } from "../ui/BlockArt";
import { BLOCK_BODY, BLOCK_ACCENT } from "../ui/blockart/blockColors.generated";
import { CAB_COVERING } from "./cabCovering.generated";

// One catalog row: [fenderId, terse caption, icon, chassis tone, full name,
// blurb, available?]. icon/tone are validated against the renderer's id unions
// at compile time, so a typo in this 300-row table is a tsc error.
type BlockRow = readonly [
  id: string,
  short: string,
  icon: IconId,
  tone: ToneId,
  name: string,
  blurb: string,
  available?: boolean,
];
interface CatalogCategory {
  key: string;
  label: string;
  blurb: string;
  blocks: readonly BlockRow[];
}

const TMP_CATALOG: readonly CatalogCategory[] = [
  {
    key: "combo",
    label: "Combo Amps",
    blurb: "Modeled combo amplifiers (no separate cab).",
    blocks: [
      [
        "ACD_TweedDeluxe",
        "57DLX",
        "combo",
        "tweed",
        "FENDER '57 DELUXE",
        "Fender '57 Deluxe (5E3 tweed)",
      ],
      [
        "ACD_TM59Bassman",
        "59BASS",
        "combo",
        "tweed",
        "FENDER '59 BASSMAN",
        "Fender '59 Bassman (tweed 4x10)",
      ],
      [
        "ACD_TMCust59Bassman",
        "59BASS-C",
        "combo",
        "tweed",
        "FENDER '59 BASSMAN CUSTOM",
        "Fender '59 Bassman (modified, Greenbacks)",
      ],
      [
        "ACD_Princeton6G2",
        "62PRN",
        "combo",
        "brownface",
        "FENDER '62 PRINCETON",
        "Fender '62 Princeton (brownface 6G2)",
      ],
      [
        "ACD_PrincetonReverb65NoFx",
        "65PRN",
        "combo",
        "blackface",
        "FENDER '65 PRINCETON REVERB",
        "Fender '65 Princeton Reverb (blackface)",
      ],
      [
        "ACD_DeluxeReverb65NoFx",
        "65DLX",
        "combo",
        "blackface",
        "FENDER '65 DELUXE REVERB",
        "Fender '65 Deluxe Reverb (blackface)",
      ],
      [
        "ACD_DeluxeReverb65BlondeVibratoNoFx",
        "65DLX-BL",
        "combo",
        "blonde",
        "FENDER '65 DELUXE REVERB BLONDE NBC",
        "Fender '65 Deluxe Reverb (blonde, no bright cap)",
      ],
      [
        "ACD_TMSuperReverbNoFx",
        "65SUP",
        "combo",
        "blackface",
        "FENDER '65 SUPER REVERB",
        "Fender '65 Super Reverb (blackface 4x10)",
      ],
      [
        "ACD_TwinReverb65NoFx",
        "65TWN",
        "combo",
        "blackface",
        "FENDER '65 TWIN REVERB",
        "Fender '65 Twin Reverb (blackface)",
      ],
      [
        "ACD_TwinReverb65BlondeNoFx",
        "65TWN-BL",
        "combo",
        "blonde",
        "FENDER '65 TWIN REVERB BLONDE",
        "Fender '65 Twin Reverb (blonde, G12-65 Creamback)",
      ],
      [
        "ACD_Bassbreaker15",
        "BBRK",
        "combo",
        "blackface",
        "FENDER BASSBREAKER",
        "Fender Bassbreaker 15",
      ],
      [
        "ACD_BluesJrIV",
        "BJR",
        "combo",
        "blackface",
        "FENDER BLUES JUNIOR",
        "Fender Blues Junior IV",
      ],
      [
        "ACD_BluesJrIVLTD",
        "BJR-LTD",
        "combo",
        "tweed",
        "FENDER BLUES JUNIOR LTD",
        "Fender Blues Junior IV LTD (Jensen C12N)",
      ],
      [
        "ACD_VibroKing",
        "VIBKNG",
        "combo",
        "blonde",
        "FENDER VIBRO-KING",
        "Fender Vibro-King",
      ],
      [
        "ACD_JTM45TremCombo",
        "JTM45-C",
        "combo",
        "bluesbreaker",
        "BRIT BREAKER",
        "Marshall JTM45 'Bluesbreaker' combo (1962)",
      ],
      [
        "ACD_AC30Brilliant",
        "AC30-TB",
        "combo",
        "vox",
        "UK 30 BRILLIANT",
        "Vox AC30 (Brilliant / Top Boost channel)",
      ],
      [
        "ACD_AC30Normal",
        "AC30-N",
        "combo",
        "vox",
        "UK 30 NORMAL",
        "Vox AC30 (Normal channel)",
      ],
      [
        "ACD_MarkIICClassA",
        "MK2C-CL",
        "combo",
        "mesa",
        "MARKSMAN CH1",
        "Mesa/Boogie Mark IIC+ (clean channel)",
      ],
      [
        "ACD_JC120",
        "JC120",
        "combo",
        "roland",
        "JC CLEAN",
        "Roland JC-120 Jazz Chorus (clean channel)",
      ],
    ],
  },

  {
    key: "halfstack",
    label: "Half Stacks",
    blurb: "Each amp head shown on its default Tone Master Pro cabinet.",
    blocks: [
      [
        "ACD_Bassbreaker15High",
        "BBRK-H",
        "combo",
        "blackface",
        "FENDER BASSBREAKER HIGH",
        "Fender Bassbreaker 15 (high-gain voicing)",
      ],
      [
        "ACD_Bassbreaker15Med",
        "BBRK-M",
        "combo",
        "blackface",
        "FENDER BASSBREAKER MED",
        "Fender Bassbreaker 15 (medium-gain voicing)",
        false,
      ],
      [
        "ACD_JTM45Head",
        "JTM45",
        "amp",
        "marshall",
        "BRITISH 45",
        "Marshall JTM45 head",
      ],
      [
        "ACD_MarshallPlexi",
        "PLEXI",
        "amp",
        "marshall",
        "BRITISH PLEXI",
        "Marshall 1959 Super Lead 100 ('Plexi')",
      ],
      [
        "ACD_JCM800TMS",
        "JCM800",
        "amp",
        "marshall",
        "BRITISH 800",
        "Marshall JCM800 2204 (50W)",
      ],
      [
        "ACD_Jubilee",
        "JUB-CL",
        "amp",
        "jubilee",
        "BRITISH JUBILEE CLEAN",
        "Marshall Silver Jubilee 2553 (clean channel)",
      ],
      [
        "ACD_JubileeClip",
        "JUB-RH",
        "amp",
        "jubilee",
        "BRITISH JUBILEE RHYTHM",
        "Marshall Silver Jubilee 2553 (rhythm/clip channel)",
      ],
      [
        "ACD_JubileeLead",
        "JUB-LD",
        "amp",
        "jubilee",
        "BRITISH JUBILEE LEAD",
        "Marshall Silver Jubilee 2553 (lead channel)",
      ],
      [
        "ACD_HiwattDR103CanMod",
        "DR103",
        "amp",
        "hiwatt",
        "HIWAY 105",
        "Hiwatt DR105 (Canadian-import DR103)",
      ],
      [
        "ACD_BE100",
        "BE100",
        "amp",
        "friedman",
        "FBE-100",
        "Friedman BE-100 (lead channel)",
      ],
      [
        "ACD_Evh100SGreen",
        "5150H-G",
        "amp",
        "boutique",
        "EVH 5150IIIS 6L6 GREEN",
        "EVH 5150IIIS 6L6 (green / low-gain channel)",
      ],
      [
        "ACD_Evh100SBlue",
        "5150H-B",
        "amp",
        "boutique",
        "EVH 5150IIIS 6L6 BLUE",
        "EVH 5150IIIS 6L6 (blue / medium-gain channel)",
      ],
      [
        "ACD_Evh100SRed",
        "5150H-R",
        "amp",
        "boutique",
        "EVH 5150IIIS 6L6 RED",
        "EVH 5150IIIS 6L6 (red / high-gain channel)",
      ],
      [
        "ACD_SLO100",
        "SLO100",
        "amp",
        "boutique",
        "SOLO 100 OVERDRIVE",
        "Soldano SLO-100",
      ],
      [
        "ACD_OrangeRockerverb50MKIII",
        "RV50",
        "amp",
        "orange",
        "TANGERINE RV53",
        "Orange Rockerverb 50 MkIII",
      ],
      [
        "ACD_MarkIICClassAB",
        "MK2C-LD",
        "amp",
        "mesa",
        "MARKSMAN CH2",
        "Mesa/Boogie Mark IIC+ (lead channel)",
      ],
      [
        "ACD_DualRectifier",
        "RECTO",
        "amp",
        "recto",
        "DOUBLE WRECK",
        "Mesa/Boogie Dual Rectifier Solo head",
      ],
      [
        "ACD_DiezelVh4Ch3",
        "VH4",
        "amp",
        "boutique",
        "PETROL",
        "Diezel VH4 (channel 3)",
      ],
      [
        "ACD_Uberschall",
        "UBER",
        "amp",
        "boutique",
        "UBER",
        "Bogner Uberschall (lead channel)",
      ],
    ],
  },

  {
    key: "bass",
    label: "Bass Amps",
    blurb: "Modeled bass amplifiers and heads.",
    blocks: [
      [
        "ACD_TMBassmanTV",
        "BASSTV",
        "combo",
        "tweed",
        "BASSMAN TV",
        "Fender Bassman TV (1x15 bass combo)",
      ],
      [
        "ACD_SuperBassmanVintage",
        "SBASS-V",
        "amp",
        "blackface",
        "FENDER SUPER BASSMAN VINTAGE",
        "Fender Super Bassman (Vintage channel)",
      ],
      [
        "ACD_SuperBassman",
        "SBASS-OD",
        "amp",
        "blackface",
        "FENDER SUPER BASSMAN OVERDRIVE",
        "Fender Super Bassman (Overdrive channel)",
      ],
      [
        "ACD_Redhead",
        "REDHD",
        "combo",
        "swr",
        "SWR REDHEAD",
        "SWR Redhead (bass combo)",
      ],
      [
        "ACD_SVTBlueLine",
        "SVT",
        "amp",
        "svt",
        "RAMPAGE BLUELINE",
        "Ampeg SVT 'Blue Line' (300W head)",
      ],
      [
        "ACD_Ampeg66B15",
        "B-15",
        "amp",
        "b15",
        "'66 FLIP TOP",
        "Ampeg B-15 'flip top' (1966)",
      ],
      [
        "ACD_GK800RB",
        "400RB",
        "amp",
        "gk",
        "ROCK-BOTTOM 400",
        "Gallien-Krueger 400RB",
      ],
    ],
  },

  {
    key: "preamp",
    label: "Preamps",
    blurb: "Acoustic & studio preamps (no power amp or speaker).",
    blocks: [
      [
        "ACD_Acoustasonic",
        "ACOUST",
        "amp",
        "acoustasonic",
        "FENDER ACOUSTASONIC",
        "Fender Acoustasonic preamp",
      ],
      [
        "ACD_StudioPreamp",
        "STUPRE",
        "rack",
        "chrome",
        "STUDIO PREAMP",
        "Clean studio mixing-desk preamp (uncolored)",
      ],
      [
        "ACD_StudioTubePreamp",
        "TUBEPR",
        "racktube",
        "slate",
        "TUBE PREAMP",
        "Tube console-style studio preamp",
      ],
    ],
  },

  {
    key: "cab",
    label: "Cabinets",
    blurb: "Factory speaker-cabinet impulse responses.",
    blocks: [
      [
        "ACD_FenBrownPrincetonJenC10R",
        "1X10-62",
        "cab1",
        "brownface",
        "1X10 '62 PRINCETON C10R",
        "Fender '62 Princeton 1x10 (Jensen C10R)",
      ],
      [
        "ACD_FenPrincetonRvb_jensen",
        "1X10-65",
        "cab1",
        "blackface",
        "1X10 '65 PRINCETON C10R",
        "Fender '65 Princeton Reverb 1x10 (Jensen C10R)",
      ],
      [
        "ACD_Fen57CustomDeluxe_eminence",
        "1X12-57",
        "cab1",
        "tweed",
        "1X12 '57 DELUXE",
        "Fender '57 Custom Deluxe 1x12 (Eminence alnico)",
      ],
      [
        "ACD_Fen57DlxBlue",
        "57-BLUE",
        "cab1",
        "tweed",
        "1X12 '57 DELUXE ALNICO BLUE",
        "Fender '57 Custom Deluxe 1x12 (Celestion Blue alnico)",
      ],
      [
        "ACD_FenBrownPrincetonCS12",
        "62PR-12",
        "cab1",
        "brownface",
        "1X12 '62 PRINCETON CS",
        "Fender '62 Princeton 1x12 (Eminence Special Design)",
      ],
      [
        "ACD_FenDlxRvb_c12k",
        "65DL-K",
        "cab1",
        "blackface",
        "1X12 '65 DELUXE C12K",
        "Fender '65 Deluxe Reverb 1x12 (Jensen C12K)",
      ],
      [
        "ACD_FenDlxRvbTMCream_g12neo",
        "65DL-CB",
        "cab1",
        "wine",
        "1X12 '65 DELUXE CREAMBACK",
        "Fender '65 Deluxe Reverb 1x12 (Celestion Creamback)",
      ],
      [
        "ACD_Fen65DlxJBLD120f",
        "65DL-JBL",
        "cab1",
        "blackface",
        "1X12 DELUXE D120",
        "Fender '65 Deluxe Reverb 1x12 (JBL D120F)",
      ],
      [
        "ACD_Fen_Bassbreaker_Celestion_G12V_70",
        "BBRK-12",
        "cab1",
        "blackface",
        "1X12 BASSBREAKER",
        "Fender Bassbreaker 15 1x12 (Celestion V-Type)",
      ],
      [
        "ACD_BlsJrIV_CelestionTypeA",
        "BJR-A",
        "cab1",
        "blackface",
        "1X12 BLUES JUNIOR A-TYPE",
        "Fender Blues Junior 1x12 (Celestion A-Type)",
      ],
      [
        "ACD_BlsJrIV_Jensen_C12N",
        "BJR-N",
        "cab1",
        "tweed",
        "1X12 BLUES JUNIOR LTD C12N",
        "Fender Blues Junior LTD 1x12 (Jensen C12N)",
      ],
      [
        "ACD_Mbg_MkIICPlus_EVM12L",
        "MES-EVM",
        "cab1",
        "mesa",
        "1X12 MEGA EV",
        "Mesa/Boogie Mark IIC+ 1x12 (EVM12L)",
      ],
      [
        "ACD_FenTwnRvb_Jensen_C12K",
        "65TW-K",
        "cab2",
        "blackface",
        "2X12 '65 TWIN C12K",
        "Fender '65 Twin Reverb 2x12 (Jensen C12K)",
      ],
      [
        "ACD_FenTwnRvbTMCream_g12neo",
        "65TW-CB",
        "cab2",
        "wine",
        "2X12 '65 TWIN CREAMBACK",
        "Fender '65 Twin Reverb 2x12 (Celestion Creamback)",
      ],
      [
        "ACD_Fen65TwinJBLD120f",
        "65TW-JBL",
        "cab2",
        "blackface",
        "2X12 TWIN D120",
        "Fender '65 Twin Reverb 2x12 (JBL D120F)",
      ],
      [
        "ACD_RolJazzChorus_JC120",
        "JC-212",
        "cab2",
        "roland",
        "2X12 JC",
        "Roland JC-120 Jazz Chorus 2x12 (open-back)",
      ],
      [
        "ACD_Mar212Cent100",
        "M2-A100",
        "cab2",
        "bluesbreaker",
        "2X12 BRITISH ALNICO 100",
        "Marshall 'Bluesbreaker' 2x12 (Celestion Alnico 100)",
      ],
      [
        "ACD_British212Greenback",
        "M2-GB",
        "cab2",
        "bluesbreaker",
        "2X12 BRITISH GREENBACK",
        "Marshall 'Bluesbreaker' 2x12 (Celestion G12M Greenback)",
      ],
      [
        "ACD_Msh_Jubilee_Slant_Celestion_G12Vintage",
        "M2-JUB",
        "cab2",
        "jubilee",
        "2X12 BRITISH JUBILEE",
        "Marshall closed-back slant 2x12 (Celestion G12-75)",
      ],
      [
        "ACD_VoxAC30_Celestion_G12_Alinco_Blue",
        "AC30-BLU",
        "cab2",
        "vox",
        "2X12 UK30 ALNICO BLUE",
        "Vox AC30 2x12 (Celestion Alnico Blue)",
      ],
      [
        "ACD_VoxAC30_Celestion_Greenback",
        "AC30-GB",
        "cab2",
        "vox",
        "2X12 UK30 GREENBACK",
        "Vox AC30 2x12 (Celestion Greenback)",
      ],
      [
        "ACD_FenVibroKing_2x12_v30",
        "VK-V30",
        "cab2",
        "wine",
        "2X12 VIBRO-KING V30",
        "Fender Vibro-King closed-back 2x12 (Celestion V30)",
      ],
      [
        "ACD_FenVibKng_p10rf",
        "VK-3X10",
        "cab3",
        "wine",
        "3X10 VIBRO-KING",
        "Fender Vibro-King 3x10 (Jensen P10R)",
      ],
      [
        "ACD_FenBassman_Jensen_Special",
        "59BS-410",
        "cab4",
        "tweed",
        "4X10 '59 BASSMAN",
        "Fender '59 Tweed Bassman 4x10 (Jensen P10R)",
      ],
      [
        "ACD_FenBassmanGB",
        "59BS-GB",
        "cab4",
        "tweed",
        "4X10 '59 BASSMAN GREENBACK",
        "Fender '59 Bassman 4x10 (Celestion G10 Greenback)",
      ],
      [
        "ACD_FenSuperRvb_p10r",
        "65SUP-10",
        "cab4",
        "blackface",
        "4X10 '65 SUPER REVERB",
        "Fender Super Reverb 4x10 (Jensen P10R)",
      ],
      [
        "ACD_FenBassBreaker412V30",
        "BBRK-412",
        "cab4",
        "blackface",
        "4X12 BASSBREAKER V30",
        "Fender Bassbreaker closed-back 4x12 (Celestion V30)",
      ],
      [
        "ACD_Mar412Cent100",
        "M4-A100",
        "cab4",
        "marshallvint",
        "4X12 BRITISH ALNICO 100",
        "Marshall 1960A 4x12 (Celestion Alnico 100)",
      ],
      [
        "ACD_MarG12H30BB",
        "M4-BLK",
        "cab4",
        "marshall",
        "4X12 BRITISH BLACKBACK",
        "Marshall 1960A 4x12 (1979 G12H 30W blackback)",
      ],
      [
        "ACD_Mar1960tvGB",
        "M4-GB",
        "cab4",
        "marshall",
        "4X12 BRITISH GREENBACK",
        "Marshall 1960TV tall 4x12 (Celestion Greenback)",
      ],
      [
        "ACD_Mar412G1265",
        "M4-G65",
        "cab4",
        "marshall",
        "4X12 BRITISH G65",
        "Marshall 1960A 4x12 (Celestion G12-65)",
      ],
      [
        "ACD_Mar1960aV30Alt",
        "M4-V30",
        "cab4",
        "marshall",
        "4X12 BRITISH V30",
        "Marshall 1960A 4x12 (Celestion Vintage 30)",
      ],
      [
        "ACD_Mar1960aV30",
        "M4-JV30",
        "cab4",
        "marshall",
        "4X12 BRITISH JUBILEE V30",
        "Marshall 1960A late-'80s closed-back 4x12 (V30)",
      ],
      [
        "ACD_Marshall_JCM800_1960A",
        "M4-T75",
        "cab4",
        "marshall",
        "4X12 BRITISH T75",
        "Marshall 1960A late-'80s 4x12 (Celestion G12T-75)",
      ],
      [
        "ACD_Evh5150iii_4x12_Celestion_G12_EVH",
        "5150-EVH",
        "cab4",
        "evhblack",
        "4X12 EVH 5150IIIS",
        "EVH 5150IIIS closed-back 4x12 (Celestion G12 EVH)",
      ],
      [
        "ACD_Fried_BE4x12_Greenback_V30",
        "FRD-GB",
        "cab4",
        "friedman",
        "4X12 FREEMAN GREENBACK",
        "Friedman 412 4x12 (Celestion Greenback)",
      ],
      [
        "ACD_Fried_BE4x12_V30_Greenback",
        "FRD-V30",
        "cab4",
        "friedman",
        "4X12 FREEMAN V30",
        "Friedman 412 4x12 (Celestion Vintage 30)",
      ],
      [
        "ACD_Hiwatt412Fane",
        "HIW-412",
        "cab4",
        "hiwatt",
        "4X12 HIWAY",
        "Hiwatt 4x12 (Fane)",
      ],
      [
        "ACD_Mbg412HBBSClosed",
        "MES-SHD",
        "cab4",
        "mesa",
        "4X12 MEGA SHADOW",
        "Mesa/Boogie 1985 half-back 4x12 (Black Shadow)",
      ],
      [
        "ACD_Mbg_RectifierTrad_CelestionV30",
        "MES-V30",
        "cab4",
        "mesa",
        "4X12 MEGA V30",
        "Mesa/Boogie Rectifier oversized 4x12 (V30)",
      ],
      [
        "ACD_Diezel412FV",
        "DZL-V30",
        "cab4",
        "boutique",
        "4X12 PETROL V30",
        "Diezel 4x12 (Celestion Vintage 30)",
      ],
      [
        "ACD_Sol_4x12_Slant_G12H",
        "SLO-412",
        "cab4",
        "boutique",
        "4X12 SOLO G12H",
        "Soldano slant 4x12 (Celestion G12H-30)",
      ],
      [
        "ACD_OrangePPC412",
        "OR-V30",
        "cab4",
        "orange",
        "4X12 TANGERINE V30",
        "Orange PPC412 straight 4x12 (Celestion V30)",
      ],
      [
        "ACD_Bogner_412STU_Celestion_G12H",
        "BOG-T75",
        "cab4",
        "boutique",
        "4X12 UBER T75",
        "Bogner Uberkab 4x12 (Celestion G12T-75)",
      ],
      [
        "ACD_Bogner_412STU_Celestion_G12_V30",
        "BOG-V30",
        "cab4",
        "boutique",
        "4X12 UBER V30",
        "Bogner Uberkab 4x12 (Celestion Vintage 30)",
      ],
      [
        "ACD_Redhead210",
        "SWR-210",
        "cab2",
        "bass",
        "2X10 SWR REDHEAD",
        "SWR Redhead combo 2x10 (Eminence)",
      ],
      [
        "ACD_GKCX410",
        "GK-410",
        "cab4",
        "bass",
        "4X10 ROCK BOTTOM",
        "Gallien-Krueger 4x10 (GK)",
      ],
      [
        "ACD_FenBassman410Neo",
        "BM-410N",
        "cab4",
        "fenderbass",
        "4X10 BASSMAN PRO NEO",
        "Fender Bassman 410 Neo 4x10 (Eminence neo)",
      ],
      [
        "ACD_FenBassman610Neo",
        "BM-610N",
        "cab6",
        "fenderbass",
        "6X10 BASSMAN PRO NEO",
        "Fender Bassman 610 Neo 6x10 (Eminence neo)",
      ],
      [
        "ACD_FenBassman810Neo",
        "BM-810N",
        "cab8",
        "fenderbass",
        "8X10 BASSMAN PRO NEO",
        "Fender Bassman 810 Neo 8x10 (Eminence neo)",
      ],
      [
        "ACD_Ampeg810E",
        "SVT-810",
        "cab8",
        "ampeg",
        "8X10 RAMPAGE BASS",
        "Ampeg SVT-810 8x10",
      ],
      [
        "ACD_FenBassman115Neo",
        "BM-115N",
        "cab15",
        "fenderbass",
        "1X15 BASSMAN PRO NEO",
        "Fender Bassman 115 Neo 1x15 (Eminence neo)",
      ],
      [
        "ACD_BassmanTV115",
        "BTV-115",
        "cab15",
        "tweed",
        "1X15 BASSMAN TV",
        "Fender Bassman TV 1x15 (Celestion)",
      ],
      [
        "ACD_FlipTop",
        "B15-115",
        "cab15",
        "fliptop",
        "1X15 RAMPAGE BASS",
        "Ampeg B-15 'flip top' 1x15 (CTS)",
      ],
      // — firmware / Pro Control only (not in printed Model Guide 1.7) —
      [
        "ACD_CabSimTMS",
        "CAB-IR",
        "cab1",
        "ink",
        "CABINET (IR BLOCK)",
        "Fender — cabinet / IR block container",
        false,
      ],
      [
        "ACD_British212AlnicoBlue",
        "M2-BLU",
        "cab2",
        "marshall",
        "2X12 BRITISH ALNICO BLUE",
        "Marshall 'Bluesbreaker' 2x12 (Celestion Alnico Blue)",
        false,
      ],
      [
        "ACD_FenSuperSonic100_412SLST_v30",
        "SS-412",
        "cab4",
        "blackface",
        "4X12 SUPER-SONIC",
        "Fender Super-Sonic 100 4x12 (Celestion V30)",
        false,
      ],
      [
        "ACD_Evh5150iii_412st",
        "5150-STR",
        "cab4",
        "boutique",
        "4X12 EVH 5150III (STRAIGHT)",
        "EVH 5150III 4x12 straight cabinet",
        false,
      ],
      [
        "ACD_FenBluesJrLTD_JensenSpecial",
        "BJR-JS",
        "cab1",
        "tweed",
        "1X12 BLUES JR LTD (JENSEN)",
        "Fender Blues Jr LTD 1x12 (Jensen Special Design)",
        false,
      ],
      [
        "ACD_Kasino_CTS_137_7014",
        "KAS-412",
        "cab4",
        "black",
        "4X12 KASINO CTS",
        "Kustom/Kasino 4x12 (CTS 137-7014)",
        false,
      ],
      [
        "ACD_Peavey5150_4x12_Sheffield_1200",
        "PV-412",
        "cab4",
        "black",
        "4X12 PEAVEY 5150 SHEFFIELD",
        "Peavey 5150 4x12 (Sheffield 1200)",
        false,
      ],
    ],
  },

  {
    key: "drive",
    label: "Stompbox / Drive",
    blurb: "Boost, overdrive, distortion, fuzz.",
    blocks: [
      [
        "ACD_Boost",
        "BOOST",
        "boost",
        "chrome",
        "BOOST",
        "Fender original (clean boost)",
      ],
      [
        "ACD_Rangemaster",
        "RANGE",
        "boost",
        "chrome",
        "RANGER BOOST",
        "Dallas Rangemaster Treble Booster",
      ],
      [
        "ACD_RackPreamp",
        "SDD3000",
        "knobs2",
        "navy",
        "SDD-BOOST",
        "Korg SDD-3000 (preamp section)",
      ],
      [
        "ACD_EPBooster",
        "EP-BST",
        "boost",
        "black",
        "XO BOOST",
        "Xotic EP Booster (Echoplex EP-3 preamp)",
      ],
      [
        "ACD_BluesBreaker",
        "BLUESBK",
        "od",
        "black",
        "BLUES MAKER",
        "Marshall Bluesbreaker",
      ],
      [
        "ACD_KingOfTone",
        "KOT",
        "od",
        "plum",
        "ROYAL TONE",
        "Analogman Prince of Tone",
      ],
      [
        "ACD_TimmyV3",
        "TIMMY",
        "od",
        "blue",
        "ENCHANTER",
        "Paul Cochrane Timmy 15th Anniversary V3",
      ],
      [
        "ACD_TubeScreamer",
        "TS808",
        "od",
        "green",
        "GREENBOX 8",
        "Ibanez TS808 Tube Screamer",
      ],
      [
        "ACD_Greenbox10",
        "TS-10",
        "od",
        "green",
        "GREENBOX 10",
        "Ibanez TS-10 Tube Screamer",
      ],
      [
        "ACD_BluesDriver",
        "BD-2",
        "od",
        "blue",
        "SAPPHIRE DRIVE",
        "Boss BD-2 Blues Driver",
      ],
      [
        "ACD_MythicDrive",
        "KLON",
        "knobs3",
        "gold",
        "MYTHIC DRIVE",
        "Klon Centaur",
      ],
      [
        "ACD_KlonCentaur",
        "KLON-S",
        "knobs3",
        "chrome",
        "MYTHIC DRIVE II",
        "Klon Centaur 'No Horse' (silver)",
      ],
      [
        "ACD_NobelsOdr1",
        "ODR-1",
        "od",
        "green",
        "NASHVILLE OVERDRIVE",
        "Nobels ODR-1BC",
      ],
      [
        "ACD_TubeDriver",
        "TUBEDR",
        "knobs4",
        "muff",
        "TUBE OD",
        "B.K. Butler Tube Driver",
      ],
      [
        "ACD_SD1",
        "SD-1",
        "od",
        "yellow",
        "SUPERDRIVE",
        "Boss SD-1 Super Overdrive",
      ],
      ["ACD_DS1", "DS-1", "od", "orange", "ORANGEBOX", "Boss DS-1 Distortion"],
      [
        "ACD_DistortionPlus",
        "DIST+",
        "knobs2",
        "yellow",
        "HARD-CLIP DISTORTION",
        "MXR Distortion+",
      ],
      [
        "ACD_Palladium",
        "PALLAD",
        "knobs6",
        "graphite",
        "MAXIMUS DRIVE",
        "Seymour Duncan Palladium Gain Stage",
      ],
      [
        "ACD_Pugilist",
        "PUGIL",
        "knobs6",
        "yellow",
        "PUGILIST DISTORTION",
        "Fender original (dual-engine distortion)",
      ],
      [
        "ACD_ObsessiveDrive",
        "OCD",
        "od",
        "muff",
        "CSD (COMIC SANS DRIVE)",
        "Fulltone OCD",
      ],
      [
        "ACD_ZenDrive",
        "ZENDRV",
        "knobs4",
        "cream",
        "NAMASTE DRIVE",
        "Hermida Audio Zendrive",
      ],
      [
        "ACD_JRockettDude",
        "DUDE",
        "knobs4",
        "black",
        "ROCKET MAN",
        "J. Rockett The Dude",
      ],
      ["ACD_Rat", "RAT", "od", "rat", "RODENT", "Pro Co RAT"],
      [
        "ACD_BigFuzz",
        "BMP-NYC",
        "bigmuff",
        "chrome",
        "BIG APPLE FUZZ",
        "Electro-Harmonix NYC Big Muff Pi",
      ],
      [
        "ACD_GreenRussianBmp",
        "BMP-RUSS",
        "bigmuff",
        "olive",
        "BIG GREEN FUZZ",
        "Electro-Harmonix Green Russian Big Muff Pi",
      ],
      [
        "ACD_RamsHeadBmp",
        "BMP-RH",
        "bigmuff",
        "ramshead",
        "BIG HORN FUZZ",
        "Electro-Harmonix Ram's Head Big Muff Pi",
      ],
      [
        "ACD_GeFuzzFace",
        "FF-GE",
        "roundfuzz",
        "fuzzface",
        "ROUND FUZZ (GERMANIUM)",
        "Dallas Arbiter Fuzz Face (germanium)",
      ],
      [
        "ACD_RoundFuzz",
        "FF-SI",
        "roundfuzz",
        "siliconfuzz",
        "ROUND FUZZ (SILICON)",
        "Dallas Arbiter Fuzz Face (silicon)",
      ],
      [
        "ACD_VariFuzz",
        "TWEAK",
        "fuzz",
        "yellow",
        "VARI FUZZ",
        "Seymour Duncan Tweak Fuzz",
      ],
      [
        "ACD_Octavia",
        "OCTAVIA",
        "fuzz",
        "blue",
        "OCTAVE FUZZ",
        "Tycobrahe Octavia",
      ],
      [
        "ACD_Octobot",
        "OCTBOT",
        "fuzz",
        "ice",
        "OCTOBOT",
        "Fender original (octave-up/down fuzz)",
      ],
      [
        "ACD_ModernBassOverdrive",
        "BDDI",
        "knobs6",
        "black",
        "BASS OVERDRIVE",
        "Tech 21 SansAmp (Bass Driver)",
      ],
    ],
  },

  {
    key: "mod",
    label: "Modulation",
    blurb: "Chorus, vibrato, flanger, phaser, tremolo, rotary, vibe.",
    blocks: [
      [
        "ACD_ChorusCE2",
        "CE-2",
        "chorus",
        "cyan",
        "ANALOG CHORUS",
        "Boss CE-2 Chorus",
      ],
      [
        "ACD_ChorusCE3",
        "CE-2 ST",
        "chorus",
        "cyan",
        "STEREO ANALOG CHORUS",
        "Boss CE-2 Chorus (stereo)",
      ],
      [
        "ACD_ChorusCE2B",
        "CE-2B",
        "chorus",
        "cyan",
        "ANALOG BASS CHORUS",
        "Boss CE-2B Bass Chorus",
      ],
      [
        "ACD_Chorus_CE5Mono",
        "CE-5",
        "chorus",
        "cyan",
        "CHORUS",
        "Boss CE-5 Chorus",
      ],
      [
        "ACD_Chorus_CE5Stereo",
        "CE-5 ST",
        "chorus",
        "cyan",
        "CHORUS STEREO",
        "Boss CE-5 Chorus (stereo)",
      ],
      [
        "ACD_DimensionChorus",
        "DC-2",
        "chorus",
        "purple",
        "3D CHORUS",
        "Boss DC-2 Dimension C",
      ],
      [
        "ACD_JC120ChorusEffect",
        "JC-CHO",
        "chorus",
        "roland",
        "JC CHORUS",
        "Roland JC-120 Jazz Chorus (chorus)",
      ],
      [
        "ACD_JC120VibratoEffect",
        "JC-VIB",
        "univibe",
        "roland",
        "JC VIBRATO",
        "Roland JC-120 Jazz Chorus (vibrato)",
      ],
      [
        "ACD_TriStereoChorus",
        "TRI-SC",
        "chorus",
        "black",
        "TRIPLE-DOUBLE CHORUS",
        "Dyno My Piano Tri-Stereo Chorus",
      ],
      [
        "ACD_VibratoVB2",
        "VB-2",
        "univibe",
        "blue",
        "ANALOG VIBRATO",
        "Boss VB-2 Vibrato",
      ],
      [
        "ACD_Flanger2p0Mono",
        "BF-3",
        "flanger",
        "purple",
        "FLANGER",
        "Boss BF-3 Flanger",
      ],
      [
        "ACD_Flanger2p0Stereo",
        "BF-3 ST",
        "flanger",
        "purple",
        "FLANGER STEREO",
        "Boss BF-3 Flanger (stereo)",
      ],
      [
        "ACD_ElectricMistress",
        "MISTRESS",
        "flanger",
        "chrome",
        "ELECTRIC FLANGER",
        "Electro-Harmonix Electric Mistress",
      ],
      [
        "ACD_EvhFlanger",
        "M117",
        "flanger",
        "black",
        "'70S FLANGER",
        "MXR Flanger (M117, 1970s)",
      ],
      [
        "ACD_StereoPanner",
        "ORBIT",
        "tremolo",
        "purple",
        "ORBIT STEREO PANNER",
        "Fender original (stereo auto-panner)",
      ],
      [
        "ACD_PhaserPH3",
        "PH-3",
        "phaser",
        "green",
        "PHASER",
        "Boss PH-3 Phaser",
      ],
      [
        "ACD_PhaserP90",
        "PHASE90",
        "phaser",
        "orange",
        "PHASER 90",
        "MXR Phase 90",
      ],
      [
        "ACD_SmallStone",
        "SMSTONE",
        "phaser",
        "chrome",
        "LITTLE ROCK PHASER",
        "Electro-Harmonix Small Stone",
      ],
      [
        "ACD_TremoloSine",
        "TR-2",
        "tremolo",
        "teal",
        "TREMOLO",
        "Boss TR-2 Tremolo",
      ],
      [
        "ACD_TremoloHarmonic",
        "HARMTR",
        "tremolo",
        "vinyl",
        "HARMONIC TREMOLO",
        "Fender brownface harmonic vibrato (6G-era)",
      ],
      [
        "ACD_PanoVerbTremolo",
        "HARM ST",
        "tremolo",
        "vinyl",
        "STEREO HARMONIC TREMOLO",
        "Fender brownface harmonic vibrato (stereo)",
      ],
      [
        "ACD_TMDR65Tremolo",
        "OPT-TR",
        "tremolo",
        "blackface",
        "OPTICAL TREMOLO",
        "Fender blackface photoresistor tremolo (Twin)",
      ],
      [
        "ACD_TremoloBias",
        "BIASTR",
        "tremolo",
        "blackface",
        "TUBE BIAS TREMOLO",
        "Fender tube-bias tremolo (Princeton Reverb)",
      ],
      [
        "ACD_UniVibe",
        "UNIVIBE",
        "univibe",
        "black",
        "UNI-VERSE VIBE",
        "Univox Uni-Vibe",
      ],
      [
        "ACD_PinwheelLeslie122",
        "LES122",
        "rotary",
        "wood",
        "ROTARY SPEAKER 122",
        "Leslie 122 rotary speaker",
      ],
      [
        "ACD_PinwheelLeslie147",
        "LES147",
        "rotary",
        "woodlt",
        "ROTARY SPEAKER 147",
        "Leslie 147 rotary speaker",
      ],
      [
        "ACD_TMPinwheelVibratone",
        "VIBTON",
        "rotary",
        "chrome",
        "VIBRATONE",
        "Fender Vibratone (rotating baffle)",
      ],
    ],
  },

  {
    key: "delay",
    label: "Delay",
    blurb: "Tape, BBD, digital, and ambient delays.",
    blocks: [
      [
        "ACD_SpaceEcho",
        "RE-201",
        "delay",
        "green",
        "SPACE DELAY",
        "Roland RE-201 Space Echo",
      ],
      [
        "ACD_SpaceEchoStereo",
        "RE201 ST",
        "delay",
        "green",
        "STEREO SPACE DELAY",
        "Roland RE-201 Space Echo (stereo)",
      ],
      [
        "ACD_EchoplexEP3",
        "EP-3",
        "delay",
        "black",
        "TAPE ECHO",
        "Maestro Echoplex EP-3",
      ],
      [
        "ACD_EchoplexEP3Stereo",
        "EP-3 ST",
        "delay",
        "black",
        "STEREO TAPE ECHO",
        "Maestro Echoplex EP-3 (stereo)",
      ],
      [
        "ACD_DM2",
        "DM-2",
        "delay",
        "red",
        "ANALOG DELAY",
        "Boss DM-2 (bucket-brigade)",
      ],
      [
        "ACD_EchoMachine",
        "EM5",
        "delay",
        "teal",
        "ECHOTANK",
        "Ibanez Soundtank EM5 Echomachine",
      ],
      [
        "ACD_MemoryMan",
        "DMM",
        "delay",
        "chrome",
        "MEMORY DELAY",
        "Electro-Harmonix Deluxe Memory Man",
      ],
      [
        "ACD_MemoryManStereo",
        "DMM ST",
        "delay",
        "chrome",
        "STEREO MEMORY DELAY",
        "Electro-Harmonix Deluxe Memory Man (stereo)",
      ],
      [
        "ACD_BoilerPlateMono",
        "DIGITL",
        "delay",
        "pink",
        "DIGITAL DELAY",
        "Fender original (clean digital delay)",
      ],
      [
        "ACD_BoilerPlateStereo",
        "DIG ST",
        "delay",
        "mint",
        "STEREO DIGITAL DELAY",
        "Fender original (clean digital delay, stereo)",
      ],
      [
        "ACD_HoldDelay",
        "DD-3",
        "delay",
        "chrome",
        "DIGITAL HOLD DELAY",
        "Boss DD-3 (Hold mode)",
      ],
      [
        "ACD_HoldDelayStereo",
        "DD-3 ST",
        "delay",
        "chrome",
        "STEREO DIGITAL HOLD DELAY",
        "Boss DD-3 (Hold mode, stereo)",
      ],
      [
        "ACD_ModDelay",
        "2290",
        "delay",
        "graphite",
        "STUDIO DELAY",
        "TC Electronic 2290",
      ],
      [
        "ACD_DynamicDelay",
        "2290 DYN",
        "delay",
        "teal",
        "DYNAMIC DELAY",
        "TC Electronic 2290 (dynamic/stereo)",
      ],
      [
        "ACD_HaloDelay",
        "HALO",
        "delay",
        "black",
        "AURORA DELAY",
        "Keeley Halo",
      ],
      [
        "ACD_HaloDelayStereo",
        "HALO ST",
        "delay",
        "black",
        "STEREO AURORA DELAY",
        "Keeley Halo (stereo)",
      ],
      [
        "ACD_RackDelay",
        "PCM42",
        "delay",
        "graphite",
        "ECHELON DELAY",
        "Lexicon PCM42",
      ],
      [
        "ACD_RackDelayStereo",
        "PCM42 ST",
        "delay",
        "graphite",
        "STEREO ECHELON DELAY",
        "Lexicon PCM42 (stereo)",
      ],
      [
        "ACD_MultiplyDelayMono",
        "PRIME",
        "delay",
        "graphite",
        "PRIME DELAY",
        "Lexicon Prime Time",
      ],
      [
        "ACD_MultiplyDelay",
        "PRIME ST",
        "delay",
        "graphite",
        "STEREO PRIME DELAY",
        "Lexicon Prime Time (stereo)",
      ],
      [
        "ACD_AutoSwellDelay",
        "SWELL",
        "delay",
        "teal",
        "AUTO-SWELL DELAY",
        "Fender original (swell delay)",
      ],
      [
        "ACD_TMDelayFilter",
        "ECHOFL",
        "delay",
        "cyan",
        "ECHO FILTER",
        "Fender original (resonant filter delay)",
      ],
      [
        "ACD_TMDelayFilterStereo",
        "ECHF ST",
        "delay",
        "cyan",
        "ECHO FILTER STEREO",
        "Fender original (resonant filter delay, stereo)",
      ],
      [
        "ACD_TMPingPong",
        "PNGPNG",
        "delay",
        "amber",
        "PING PONG DELAY",
        "Fender original (ping-pong stereo delay)",
      ],
      [
        "ACD_TMReverse",
        "REVRSE",
        "delay",
        "green",
        "REVERSE DELAY",
        "Fender original (reverse delay)",
      ],
      [
        "ACD_Polyhedron",
        "POLYHD",
        "delay",
        "pink",
        "POLYHEDRON PITCH DELAY",
        "Fender original (dual pitch-shift delay)",
      ],
      [
        "ACD_Glooper",
        "GLOOP",
        "delay",
        "graphite",
        "GLOOPER",
        "Fender original (glitch looper/delay)",
      ],
      [
        "ACD_Freeze",
        "FREEZE",
        "delay",
        "ice",
        "ARCTIC SUSTAINER",
        "Electro-Harmonix Freeze",
      ],
      [
        "ACD_DeepFreeze",
        "DEEPFRZ",
        "delay",
        "slate",
        "ANTARCTIC SUSTAINER",
        "Electro-Harmonix Deep Freeze",
      ],
      [
        "ACD_Doubler",
        "DOUBLR",
        "delay",
        "graphite",
        "STEREO DOUBLER",
        "Fender original (multi-track doubler)",
      ],
    ],
  },

  {
    key: "reverb",
    label: "Reverb",
    blurb: "Spring, room, hall, plate, shimmer, convolution.",
    blocks: [
      [
        "ACD_TMSpring63",
        "SPR63",
        "spring",
        "brownface",
        "'63 SPRING REVERB",
        "Fender '63 outboard spring reverb",
      ],
      [
        "ACD_TMSpring63Conv",
        "SPR63C",
        "spring",
        "brownface",
        "'63 SPRING REVERB CONVOLUTION",
        "Fender '63 spring reverb (convolution)",
      ],
      [
        "ACD_TMSpring65",
        "SPR65",
        "spring",
        "blackface",
        "'65 SPRING REVERB",
        "Fender blackface spring reverb",
      ],
      [
        "ACD_TMSpring65Conv",
        "SPR65C",
        "spring",
        "blackface",
        "'65 SPRING REVERB CONVOLUTION",
        "Fender blackface spring reverb (convolution)",
      ],
      [
        "ACD_TMSmallRoom",
        "SM-ROOM",
        "hall",
        "lake",
        "SMALL ROOM REVERB",
        "Fender original (digital room)",
      ],
      [
        "ACD_TMLargeRoom",
        "LG-ROOM",
        "hall",
        "teal",
        "LARGE ROOM REVERB",
        "Fender original (digital room)",
      ],
      [
        "ACD_TMSmallHall",
        "SM-HALL",
        "hall",
        "gold",
        "SMALL HALL REVERB",
        "Fender original (digital hall)",
      ],
      [
        "ACD_TMLargeHall",
        "LG-HALL",
        "hall",
        "green",
        "LARGE HALL REVERB",
        "Fender original (digital hall)",
      ],
      [
        "ACD_FenderSmallModulatedHall",
        "MOD-SH",
        "hall",
        "gold",
        "MODULATED SMALL HALL REVERB",
        "Fender original (modulated hall)",
      ],
      [
        "ACD_FenderLargeModulatedHall",
        "MOD-LH",
        "hall",
        "green",
        "MODULATED LARGE HALL REVERB",
        "Fender original (modulated hall)",
      ],
      [
        "ACD_TMSmallPlate",
        "EMT-SM",
        "plate",
        "red",
        "SMALL PLATE REVERB",
        "EMT 140 plate (smaller)",
      ],
      [
        "ACD_TMLargePlate",
        "EMT-LG",
        "plate",
        "muff",
        "LARGE PLATE REVERB",
        "EMT 140 plate",
      ],
      [
        "ACD_CloudReverb",
        "CLOUD",
        "shimmer",
        "chrome",
        "CLOUD REVERB",
        "Fender original (ambient pitch-modulated)",
      ],
      [
        "ACD_TMShimmer",
        "CELEST",
        "shimmer",
        "ice",
        "CELESTIAL REVERB",
        "Fender original (dual-pitch atmospheric)",
      ],
      [
        "ACD_NebulaTamed",
        "NEBULA",
        "shimmer",
        "ink",
        "NEBULA REVERB",
        "Fender original (huge ambient)",
      ],
      [
        "ACD_NebulaReverse",
        "NEB-R",
        "shimmer",
        "ink",
        "REVERSE NEBULA REVERB",
        "Fender original (reverse ambient)",
      ],
      [
        "ACD_SlimmerShimmer",
        "SHIMER",
        "shimmer",
        "teal",
        "SHIMMER REVERB",
        "Fender original (reverb + 2-octave shift)",
      ],
      [
        "ACD_TMAmbienceConv",
        "ATMOS",
        "hall",
        "graphite",
        "ATMOSPHERE CONVOLUTION ROOM REVERB",
        "Fender original (convolution room)",
      ],
      [
        "ACD_TMChamberConv",
        "VAST",
        "hall",
        "ink",
        "VAST CONVOLUTION CHAMBER REVERB",
        "Fender original (convolution chamber)",
      ],
      [
        "ACD_TMCathedralConv",
        "ASCEN",
        "hall",
        "chrome",
        "ASCENSION CONVOLUTION HALL REVERB",
        "Fender original (convolution hall)",
      ],
      [
        "ACD_TMEtherealHallConv",
        "ETHERL",
        "hall",
        "lavender",
        "ETHEREAL CONVOLUTION HALL REVERB",
        "Fender original (convolution hall)",
      ],
      [
        "ACD_TMHallOfDoomConv",
        "IMPERM",
        "hall",
        "black",
        "IMPERIUM CONVOLUTION HALL REVERB",
        "Fender original (convolution hall)",
      ],
      [
        "ACD_TMNewAgeHallConv",
        "RETROG",
        "hall",
        "ink",
        "RETROGRADE CONVOLUTION HALL REVERB",
        "Fender original (convolution hall)",
      ],
      [
        "ACD_TMWarmPlateConv",
        "ALKALN",
        "plate",
        "muff",
        "ALKALINE CONVOLUTION PLATE REVERB",
        "Fender original (convolution plate)",
      ],
      [
        "ACD_TMRichPlateConv",
        "KINETC",
        "plate",
        "chrome",
        "KINETIC CONVOLUTION PLATE REVERB",
        "Fender original (convolution plate)",
      ],
    ],
  },

  {
    key: "dyn",
    label: "Dynamics",
    blurb: "Compressors, sustain, gates, volume / swell.",
    blocks: [
      [
        "ACD_DynaComp",
        "DYNA",
        "comp",
        "red",
        "DYNAMIC COMPRESSOR",
        "MXR Dyna Comp",
      ],
      [
        "ACD_CS3",
        "CS-3",
        "comp",
        "blue",
        "PEDAL COMP",
        "Boss CS-3 Compression Sustainer",
      ],
      [
        "ACD_CompressorSimple",
        "DYNA-S",
        "comp",
        "ink",
        "SIMPLE COMPRESSOR",
        "MXR Dyna Comp (simplified)",
      ],
      [
        "ACD_CompressorSimpleSoftKnee",
        "STUCMP",
        "comp",
        "ink",
        "STUDIO COMPRESSOR",
        "Fender original (studio comp, soft-knee)",
      ],
      ["ACD_Sustain", "M163", "comp", "black", "SUSTAIN", "MXR M-163 Sustain"],
      [
        "ACD_ChromeGate",
        "DECIM",
        "gate",
        "chrome",
        "METAL GATE",
        "ISP Technologies Decimator II G String",
      ],
      [
        "ACD_NoiseGate",
        "GATE",
        "gate",
        "black",
        "NOISE GATE",
        "Fender original (noise gate)",
      ],
      [
        "ACD_NoiseGateMustang",
        "SMPGAT",
        "gate",
        "gold",
        "SIMPLE GATE",
        "Fender original (simplified noise gate)",
      ],
      [
        "ACD_VolumePedal",
        "VOL",
        "treadle",
        "ink",
        "VOLUME PEDAL",
        "Fender original (volume pedal)",
      ],
      [
        "ACD_VolumeSwell",
        "A-SWLL",
        "vol",
        "red",
        "AUTO-SWELL VOLUME",
        "Fender original (auto volume swell)",
      ],
      [
        "ACD_SlowAttack",
        "SLOWAT",
        "vol",
        "black",
        "SLOW ATTACK",
        "Electro-Harmonix POG ('Attack' parameter)",
      ],
    ],
  },

  {
    key: "eq",
    label: "EQ",
    blurb: "Graphic & parametric EQ, pass / notch filters.",
    blocks: [
      [
        "ACD_MustangFiveBandEq1",
        "EQ5-GR",
        "eq5",
        "black",
        "EQ5 GRAPHIC",
        "Mesa/Boogie Mark IIC+ 5-band graphic EQ",
      ],
      [
        "ACD_MustangSevenBandEq",
        "GE-7",
        "eq7",
        "muff",
        "EQ7 GRAPHIC",
        "Boss GE-7",
      ],
      [
        "ACD_TMBassGraphicEQ7",
        "EQ7B",
        "eq7",
        "muff",
        "EQ-7B BASS GRAPHIC",
        "Fender original (7-band bass graphic)",
      ],
      [
        "ACD_TMGraphicEQ7Wide",
        "EQ7W",
        "eq7",
        "wood",
        "EQ-7W WIDE-RANGE BASS GRAPHIC",
        "Fender original (7-band wide bass graphic)",
      ],
      [
        "ACD_TenBandEQStereo",
        "EQ10 ST",
        "eq10",
        "chrome",
        "EQ-10 GRAPHIC",
        "Fender original (10-band graphic)",
      ],
      [
        "ACD_TenBandEQMono",
        "EQ10",
        "eq10",
        "chrome",
        "EQ10 MONO",
        "Fender original (10-band graphic EQ, mono)",
      ],
      [
        "ACD_MustangPEQ",
        "EQ3-P",
        "peq",
        "chrome",
        "EQ-3 PARAMETRIC",
        "Fender original (3-band parametric)",
      ],
      [
        "ACD_FiveBandParamEQ",
        "EQ5-P",
        "screen",
        "slate",
        "EQ-5 PARAMETRIC",
        "Fender original (5-band parametric)",
      ],
      [
        "ACD_HighLowPass",
        "LO/HI",
        "screen",
        "slate",
        "LOW/HIGH CUT FILTER",
        "Fender original (dual low+high cut)",
      ],
      [
        "ACD_LowPass",
        "HICUT",
        "screen",
        "slate",
        "HIGH CUT FILTER",
        "Fender original (low-pass / high-cut)",
      ],
      [
        "ACD_HighPass",
        "LOCUT",
        "screen",
        "slate",
        "LOW CUT FILTER",
        "Fender original (high-pass / low-cut)",
      ],
      [
        "ACD_NotchFilter",
        "NOTCH",
        "screen",
        "slate",
        "NOTCH FILTER",
        "Fender original (notch filter)",
      ],
    ],
  },

  {
    key: "filter",
    label: "Filter / Wah",
    blurb: "Wahs and envelope / auto filters.",
    blocks: [
      [
        "ACD_CryBabyQ535",
        "535Q",
        "wah",
        "black",
        "CUSTOM WAH",
        "Dunlop Cry Baby 535Q",
      ],
      [
        "ACD_CryBabyGCB95",
        "GCB95",
        "wah",
        "black",
        "TEARDROP WAH",
        "Dunlop Cry Baby GCB-95",
      ],
      ["ACD_CryBabyV847", "V847", "wah", "black", "VOCAL WAH", "Vox V847 Wah"],
      [
        "ACD_MicroTronIV",
        "MUTRON",
        "envf",
        "blue",
        "FILTRON",
        "Mu-Tron Micro-Tron IV (envelope filter)",
      ],
      [
        "ACD_KorgA2AutoWah",
        "KORG-A3",
        "envf",
        "chrome",
        "ENIGMA FILTER",
        "Korg A3 (filter setting)",
      ],
      [
        "ACD_EcFilter",
        "ENVFLT",
        "envf",
        "yellow",
        "ENVELOPE FILTER",
        "Fender original (envelope filter)",
        false,
      ],
    ],
  },

  {
    key: "pitch",
    label: "Pitch",
    blurb: "Octave, detune, harmonizer, whammy, granular.",
    blocks: [
      [
        "ACD_MicroPitch",
        "MICRO",
        "octave",
        "red",
        "MICRO SHIFTER",
        "Eventide Micro Pitch",
      ],
      [
        "ACD_ChromaticPitchShifter",
        "CHROMA",
        "octave",
        "black",
        "CHROMATIC PITCH SHIFTER",
        "Fender original (chromatic pitch shift)",
      ],
      [
        "ACD_POG",
        "POG2",
        "octslider",
        "chrome",
        "POLYGON OCTAVE SHIFTER",
        "Electro-Harmonix POG2",
      ],
      [
        "ACD_PolyChord",
        "POLYVC",
        "octave",
        "chrome",
        "POLYVOICE PITCH SHIFTER",
        "Fender original (3-voice polyphonic shifter)",
      ],
      [
        "ACD_DiatonicPitchShifter",
        "DIATON",
        "octave",
        "plum",
        "DIATONIC PITCH SHIFTER",
        "Fender original (intelligent harmony)",
      ],
      [
        "ACD_Freqout",
        "FREQOUT",
        "octave",
        "black",
        "FEEDBACK GENERATOR",
        "Fender original (feedback simulator)",
      ],
      [
        "ACD_WhammyV5Detune",
        "WHAM-DT",
        "whammy",
        "red",
        "PEDAL DETUNE",
        "DigiTech Whammy (detune mode)",
      ],
      [
        "ACD_WhammyV5Classic",
        "WHAMMY",
        "whammy",
        "red",
        "PEDAL SHIFTER",
        "DigiTech Whammy (whammy mode)",
      ],
      [
        "ACD_PolyPitchShifter",
        "CAPO",
        "octave",
        "black",
        "VIRTUAL CAPO",
        "Fender original (polyphonic capo)",
      ],
      [
        "ACD_GranularArp",
        "GRANAR",
        "octave",
        "slate",
        "GRANULAR ARPEGGIATOR",
        "Fender original (granular delay/pitch)",
      ],
    ],
  },

  {
    key: "synth",
    label: "Synth",
    blurb: "Fender's own guitar-synth algorithms.",
    blocks: [
      [
        "ACD_GuitarSynth",
        "CERBRS",
        "synth",
        "orange",
        "CERBERUS POLYSYNTH",
        "Fender original (3-voice guitar polysynth)",
      ],
      [
        "ACD_GuitarSynthLite",
        "AETHON",
        "synth",
        "black",
        "AETHON POLYSYNTH",
        "Fender original (single-voice guitar synth)",
      ],
      [
        "ACD_WaveMorphSynth",
        "WAVMOR",
        "synth",
        "chrome",
        "WAVEMORPH",
        "Fender original (wave-morphing synth)",
      ],
    ],
  },

  // ── firmware 1.8 additions (41 menu-exposed new models) ──────────────────
  // icon/tone per the 1.8 Models-tab illustration handoff; ids + names from the
  // 1.8.45 model-leaks inventory. Each per-form variant id is keyed explicitly so
  // it renders its own form (head=amp icon, combo=combo icon) instead of relying
  // on resolveBlockArt's suffix-stripping. `short` carries the engine's label-keyed
  // discriminator (EVH GREEN/BLUE/RED accent · Twin 15 · gear-pedal name · 76).
  {
    key: "fw18",
    label: "Firmware 1.8",
    blurb: "Models added in Tone Master Pro firmware 1.8.",
    blocks: [
      [
        "ACD_TMChamp57",
        "57CHMP",
        "amp",
        "tweed",
        "'57 CHAMP",
        "Fender '57 Champ (5F1 tweed 1x8)",
      ],
      [
        "ACD_TMChamp57CabIR",
        "57CHMP",
        "combo",
        "tweed",
        "'57 CHAMP",
        "Fender '57 Champ (5F1 tweed 1x8)",
      ],
      [
        "ACD_PrincetonReverb68NoFx",
        "68PRN-A",
        "amp",
        "silverface",
        "'68 CUSTOM PRINCETON REVERB (AMP ONLY)",
        "Fender '68 Custom Princeton Reverb (silverface)",
      ],
      [
        "ACD_PrincetonReverb68NoFxCabIR",
        "68PRN-A",
        "combo",
        "silverface",
        "'68 CUSTOM PRINCETON REVERB (AMP ONLY)",
        "Fender '68 Custom Princeton Reverb (silverface)",
      ],
      [
        "ACD_PrincetonReverb68CabIRConvRvb",
        "68PRN",
        "combo",
        "silverface",
        "'68 CUSTOM PRINCETON REVERB",
        "Fender '68 Custom Princeton Reverb (silverface)",
      ],
      [
        "ACD_DeluxeReverb68CustomNoFx",
        "68DLX-A",
        "amp",
        "silverface",
        "'68 CUSTOM DELUXE REVERB (AMP ONLY)",
        "Fender '68 Custom Deluxe Reverb (silverface)",
      ],
      [
        "ACD_DeluxeReverb68CustomNoFxCabIR",
        "68DLX-A",
        "combo",
        "silverface",
        "'68 CUSTOM DELUXE REVERB (AMP ONLY)",
        "Fender '68 Custom Deluxe Reverb (silverface)",
      ],
      [
        "ACD_DeluxeReverb68CustomCabIRConvRvb",
        "68DLX",
        "combo",
        "silverface",
        "'68 CUSTOM DELUXE REVERB",
        "Fender '68 Custom Deluxe Reverb (silverface)",
      ],
      [
        "ACD_HypersonicAmp6L6Green",
        "5150C-G",
        "amp",
        "evhmodern",
        "EVH 5150 III 6L6 GREEN COMBO",
        "EVH 5150 III 6L6 (green / clean channel)",
      ],
      [
        "ACD_HypersonicAmp6L6GreenCabIR",
        "5150C-G",
        "combo",
        "evhmodern",
        "EVH 5150 III 6L6 GREEN COMBO",
        "EVH 5150 III 6L6 (green / clean channel)",
      ],
      [
        "ACD_HypersonicAmp6L6Blue",
        "5150C-B",
        "amp",
        "evhmodern",
        "EVH 5150 III 6L6 BLUE COMBO",
        "EVH 5150 III 6L6 (blue / crunch channel)",
      ],
      [
        "ACD_HypersonicAmp6L6BlueCabIR",
        "5150C-B",
        "combo",
        "evhmodern",
        "EVH 5150 III 6L6 BLUE COMBO",
        "EVH 5150 III 6L6 (blue / crunch channel)",
      ],
      [
        "ACD_HypersonicAmp6L6Red",
        "5150C-R",
        "amp",
        "evhmodern",
        "EVH 5150 III 6L6 RED COMBO",
        "EVH 5150 III 6L6 (red / lead channel)",
      ],
      [
        "ACD_HypersonicAmp6L6RedCabIR",
        "5150C-R",
        "combo",
        "evhmodern",
        "EVH 5150 III 6L6 RED COMBO",
        "EVH 5150 III 6L6 (red / lead channel)",
      ],
      [
        "ACD_TMRumbleV3",
        "RMB800",
        "amp",
        "bass",
        "RUMBLE 800",
        "Fender Rumble 800 (class-D bass head)",
      ],
      [
        "ACD_TMRumbleV3CabIR",
        "RMB800",
        "combo",
        "bass",
        "RUMBLE 800",
        "Fender Rumble 800 (class-D bass head)",
      ],
      [
        "ACD_TwinReverbCustom15NoFxCabIR",
        "65TW15-A",
        "combo",
        "blackface",
        "'65 TWIN CUSTOM 15 (AMP ONLY)",
        "Fender '65 Twin Reverb Custom (1x15 Eminence)",
      ],
      [
        "ACD_TwinReverbCustom15VibratoCabIRConvRvb",
        "65TW15",
        "combo",
        "blackface",
        "'65 TWIN CUSTOM 15",
        "Fender '65 Twin Reverb Custom (1x15 Eminence)",
      ],
      [
        "ACD_Fen57Champ",
        "1X8-57",
        "cab1",
        "tweed",
        "1X8 '57 CHAMP",
        "Fender '57 Champ 1x8 (tweed)",
      ],
      [
        "ACD_FenPrincGB",
        "65PR-GB",
        "cab1",
        "blackface",
        "1X10 '65 PRINCETON GB",
        "Fender '65 Princeton Reverb 1x10 (Celestion G10 Greenback)",
      ],
      [
        "ACD_Fen68PrinceG10R30",
        "68PR-10",
        "cab1",
        "silverface",
        "1X10 '68 PRINCETON",
        "Fender '68 Custom Princeton 1x10 (silverface, G10R-30)",
      ],
      [
        "ACD_Fen65DlxGB",
        "65DL-GB",
        "cab1",
        "blackface",
        "1X12 '65 DELUXE GB",
        "Fender '65 Deluxe Reverb 1x12 (Celestion G12 Greenback)",
      ],
      [
        "ACD_Fen68DlxG12V70",
        "68DL-12",
        "cab1",
        "silverface",
        "1X12 '68 DELUXE",
        "Fender '68 Custom Deluxe 1x12 (silverface, Celestion V-70)",
      ],
      [
        "ACD_Hypersonic_112",
        "5150-112",
        "cab1",
        "evhblack",
        "1X12 EVH 5150 G12H",
        "EVH 5150 1x12 (Celestion G12H)",
      ],
      [
        "ACD_FenTwinEmi15",
        "65TW-15",
        "cab15",
        "blackface",
        "1X15 TWIN CUSTOM",
        "Fender '65 Twin Custom 1x15 (Eminence Special Design)",
      ],
      [
        "ACD_Evh412G12H30",
        "M4-H30",
        "cab4",
        "marshall",
        "4X12 BRITISH G12H",
        "Marshall 1960 4x12 (Celestion G12H-30)",
      ],
      [
        "ACD_StepTremolo",
        "STEPTR",
        "steptrem",
        "slate",
        "STEP TREMOLO",
        "Fender original (step / square-wave tremolo)",
      ],
      [
        "ACD_TCIntegratedPre",
        "TC-IP",
        "labboost",
        "graphite",
        "INTEGRATOR BOOST",
        "TC Electronic Integrated Preamp (boost)",
      ],
      [
        "ACD_TCIntegratedPreStatic",
        "FORT33",
        "gruntboost",
        "black",
        "GRUNT BOOST",
        "TC Electronic Integrated Preamp (fixed-gain boost)",
      ],
      [
        "ACD_Rockman",
        "ROCKMAN",
        "rockbox",
        "black",
        "ROCKBOX 100",
        "Rockman X100 (80s pocket headphone amp)",
      ],
      [
        "ACD_Lightspeed",
        "LSPEED",
        "od3",
        "blue",
        "LIGHTYEAR",
        "Greer Lightspeed Organic Overdrive",
      ],
      [
        "ACD_Plumes",
        "PLUMES",
        "od3",
        "green",
        "PINIONS",
        "EarthQuaker Devices Plumes (overdrive)",
      ],
      [
        "ACD_Blumes",
        "BLUMES",
        "od3",
        "yellow",
        "RUNES",
        "EarthQuaker Devices Plumes (bass overdrive voicing)",
      ],
      [
        "ACD_StepFilterDelay",
        "STPFDL",
        "stepfilterdelay",
        "slate",
        "STEP FILTER DELAY",
        "Fender original (step-filter delay)",
      ],
      [
        "ACD_SpectralDelay",
        "PRISM",
        "prismdelay",
        "graphite",
        "PRISMATIC DELAY",
        "Fender original (prismatic spectral delay)",
      ],
      [
        "ACD_CirrostratusLite",
        "CIRRO",
        "cirrusverb",
        "frost",
        "CIRROSTRATUS REVERB",
        "Fender original (Cirrostratus ambient reverb)",
      ],
      [
        "ACD_Cirrostratus",
        "CIRR-SV",
        "cirrussynthverb",
        "frost",
        "CIRROSTRATUS SYNTHVERB",
        "Fender original (Cirrostratus synth reverb)",
      ],
      [
        "ACD_SpectralReverb",
        "SPECRV",
        "spectralverb",
        "chrome",
        "SPECTRAL REVERB",
        "Fender original (spectral shimmer reverb)",
      ],
      [
        "ACD_UA1176",
        "1176",
        "rack",
        "slate",
        "SEVENTY SIXER COMPRESSOR",
        "Universal Audio 1176 (FET compressor)",
      ],
      [
        "ACD_StepFilter",
        "STPFLT",
        "stepfilter",
        "slate",
        "STEP FILTER",
        "Fender original (step / sequenced filter)",
      ],
      [
        "ACD_PitchSequencer",
        "PTCHSQ",
        "pitchseq",
        "slate",
        "PITCH SEQUENCER",
        "Fender original (pitch step-sequencer)",
      ],
    ],
  },

  {
    key: "mic",
    label: "Microphones",
    blurb: "Cab-mic models (a parameter list, not DSP blocks).",
    blocks: [
      ["MIC_C414", "C414", "mic_c414", "chrome", "CONDENSER C414", "AKG C414"],
      [
        "MIC_M23",
        "M23",
        "mic_pencil",
        "chrome",
        "CONDENSER M23",
        "Earthworks Audio M23",
      ],
      [
        "MIC_MD421",
        "MD421",
        "mic_421",
        "black",
        "DYNAMIC MD421",
        "Sennheiser MD 421",
      ],
      [
        "MIC_R121",
        "R121",
        "mic_ribbon",
        "chrome",
        "RIBBON R121",
        "Royer Labs R-121",
      ],
      [
        "MIC_RE20",
        "RE20",
        "mic_re20",
        "black",
        "DYNAMIC RE20",
        "Electro-Voice RE20",
      ],
      ["MIC_SM7B", "SM7B", "mic_sm7b", "black", "DYNAMIC SM7B", "Shure SM7B"],
      [
        "MIC_SM57",
        "SM57",
        "mic_sm57",
        "graphite",
        "DYNAMIC SM57",
        "Shure SM57",
      ],
    ],
  },

  {
    key: "util",
    label: "Utility / Routing",
    blurb: "FX-loop inserts, external cab & IR routing.",
    blocks: [
      // Loops 1 & 2 are the analog (pre-A/D) instrument-path loops; 3 & 4 are
      // digital. All four share the fxloop icon, distinguished by tone.
      [
        "ACD_FxLoop1",
        "FX-1",
        "fxloop",
        "olive",
        "FX LOOP 1",
        "Fender utility — effects-loop send/return",
        false,
      ],
      [
        "ACD_FxLoop2",
        "FX-2",
        "fxloop",
        "red",
        "FX LOOP 2",
        "Fender utility — effects-loop send/return",
        false,
      ],
      [
        "ACD_FxLoop3",
        "FX-3",
        "fxloop",
        "chrome",
        "FX LOOP 3",
        "Fender utility — effects-loop send/return",
        false,
      ],
      [
        "ACD_FxLoop4",
        "FX-4",
        "fxloop",
        "blue",
        "FX LOOP 4",
        "Fender utility — effects-loop send/return",
        false,
      ],
      [
        "ACD_FxLoop3_4",
        "FX-3+4",
        "fxloop",
        "green",
        "FX LOOP 3+4 STEREO",
        "Fender utility — combined stereo effects loop",
        false,
      ],
      [
        "ACD_ExternalCab",
        "EXT-CB",
        "extcab",
        "ink",
        "EXTERNAL CABINET",
        "Fender utility — external cab / 4-cable-method",
        false,
      ],
      [
        "ACD_UserIRTMS",
        "IR",
        "ir",
        "ink",
        "IMPULSE RESPONSE",
        "Fender utility — user impulse-response (IR) loader",
        false,
      ],
    ],
  },
];

// Factory default head → cabinet pairing for each half-stack amp head, matching
// the Tone Master Pro's shipped defaults (each head with its own brand's cab).
const HALF_STACK_DEFAULTS = {
  ACD_Bassbreaker15High: "ACD_FenBassBreaker412V30",
  ACD_Bassbreaker15Med: "ACD_FenBassBreaker412V30",
  ACD_JTM45Head: "ACD_Mar1960tvGB",
  ACD_MarshallPlexi: "ACD_Mar1960tvGB",
  ACD_JCM800TMS: "ACD_Marshall_JCM800_1960A",
  ACD_Jubilee: "ACD_Mar1960aV30",
  ACD_JubileeClip: "ACD_Mar1960aV30",
  ACD_JubileeLead: "ACD_Mar1960aV30",
  ACD_HiwattDR103CanMod: "ACD_Hiwatt412Fane",
  ACD_BE100: "ACD_Fried_BE4x12_V30_Greenback",
  ACD_Evh100SGreen: "ACD_Evh5150iii_4x12_Celestion_G12_EVH",
  ACD_Evh100SBlue: "ACD_Evh5150iii_4x12_Celestion_G12_EVH",
  ACD_Evh100SRed: "ACD_Evh5150iii_4x12_Celestion_G12_EVH",
  ACD_SLO100: "ACD_Sol_4x12_Slant_G12H",
  ACD_OrangeRockerverb50MKIII: "ACD_OrangePPC412",
  ACD_MarkIICClassAB: "ACD_Mbg412HBBSClosed",
  ACD_DualRectifier: "ACD_Mbg_RectifierTrad_CelestionV30",
  ACD_DiezelVh4Ch3: "ACD_Diezel412FV",
  ACD_Uberschall: "ACD_Bogner_412STU_Celestion_G12_V30",
  // '66 Flip Top (Ampeg B-15): a separate head on a closed 1x15 cab — the
  // flip-top, not a Fender combo. Renders as a head-on-cab stack (form
  // half_stack in the Model Guide) over the B-15's own 1x15 cab.
  ACD_Ampeg66B15: "ACD_FlipTop",
};

// Per-head override of the PAIRED CAB's chassis tone for a half-stack, when the
// factory cab's catalogued tone differs from the head's livery. The Silver
// Jubilee heads ship on a SILVER cab, but their shared 4x12 cab row
// (ACD_Mar1960aV30) is catalogued black for its own standalone Cabinets listing
// — so tint just the stacked cab here, leaving the standalone cab unchanged.
export const HALF_STACK_CAB_TONE: Record<string, ToneId> = {
  ACD_Jubilee: "jubilee",
  ACD_JubileeClip: "jubilee",
  ACD_JubileeLead: "jubilee",
};

// ── Flatten → by-id map ──────────────────────────────────────────────────────
export interface BlockArtSpec {
  id: string;
  icon: string;
  tone: string;
  /** terse on-strip caption (uppercase, single line) */
  short: string;
  /** full Fender model name */
  name: string;
  fam: string;
  /** stompbox footswitch style (pedal-form blocks only) — matched per ref:
   *  plate = big black-rubber treadle plate (Boss + the metal gate + Ibanez TS-10);
   *  metal = small metallic rectangle switch (Ibanez TS808);
   *  round = chrome button (everything else, the default). */
  footswitch: "plate" | "metal" | "round";
  /** ref-derived per-block body color (pedals) — overlays the tone default in
   *  BlockArt. Sampled deterministically; see blockColors.generated.ts. */
  body?: string;
  /** Fender reverb-chassis accent (footswitch-band colour) — present iff the block
   *  is one of the 8 cream-chassis reverbs; drives the colored footswitch section. */
  accent?: string;
  /** ref-derived colour of a recessed control panel behind the knobs/sliders — set
   *  only for the few pedals whose ref shows a distinct coloured panel (the MEGA
   *  EQ-5 blue slider bed, the FILTRON blue control band). */
  panel?: string;
}

// Footswitch overrides keyed by id (the brand/ref doesn't follow a clean rule):
// the big black-rubber plate is worn by Boss pedals (detected via blurb) plus the
// metal gate + the Ibanez TS-10; the Ibanez TS808 has a small metal-rectangle switch.
// The Fender bass/parametric EQs are Boss-style enclosures (rubber treadle) whose
// blurb says "Fender original", so they need an explicit override.
const FS_PLATE = new Set([
  "ACD_ChromeGate",
  "ACD_Greenbox10",
  "ACD_NobelsOdr1",
  "ACD_TMBassGraphicEQ7",
  "ACD_TMGraphicEQ7Wide",
  "ACD_MustangPEQ",
]);
const FS_METAL = new Set(["ACD_TubeScreamer"]);
// Recessed coloured control panel behind the knobs/sliders, keyed by id (ref-sampled
// blue). The MEGA EQ-5's body is black with a blue slider bed; the FILTRON is grey
// with a blue control band — so the panel is distinct from the enclosure body.
const KNOB_PANEL: Record<string, string> = {
  ACD_MustangFiveBandEq1: "#3c6c84",
  ACD_MicroTronIV: "#24549c",
};
function footswitchOf(id: string, blurb: string): "plate" | "metal" | "round" {
  if (FS_METAL.has(id)) return "metal";
  if (FS_PLATE.has(id) || /\bBoss\b/.test(blurb)) return "plate";
  return "round";
}

const BY_ID: Record<string, BlockArtSpec | undefined> = {};
// Secondary index by full model name (first-wins), for catalog rows that carry no
// FenderId to resolve by — the 7 Microphones have block_id=null (they're cab
// parameters, not DSP blocks), so they reach their art via this name index. The
// blockArt `name` field equals the catalog `block_name` for these rows.
const BY_NAME: Record<string, BlockArtSpec> = {};
for (const cat of TMP_CATALOG) {
  for (const [id, label, icon, tone, name, blurb] of cat.blocks) {
    const art: BlockArtSpec = {
      id,
      icon,
      tone,
      short: normalizeShort(label),
      name,
      fam: cat.key,
      footswitch: footswitchOf(id, blurb),
      body: BLOCK_BODY[id],
      accent: BLOCK_ACCENT[id],
      panel: KNOB_PANEL[id],
    };
    BY_ID[id] = art;
    if (name && !(name in BY_NAME)) BY_NAME[name] = art;
  }
}

/** Terse label → strip caption: uppercase, dashes→spaces, split digit↔letter runs
 * ("57DLX" → "57 DLX") so captions read cleanly on one line. */
function normalizeShort(label: string): string {
  return label
    .toUpperCase()
    .replace(/-/g, " ")
    .replace(/(\d)([A-Z])/g, "$1 $2")
    .replace(/\s+/g, " ")
    .trim();
}

// Device FenderIds carry cab/IR/convolution suffixes the catalog id omits
// (e.g. ACD_TweedDeluxeCabIR → ACD_TweedDeluxe). Strip them one at a time,
// checking after each. NoFx is part of real base ids so it is NOT stripped.
const SUFFIX = /(ConvRvb|CabIR|NoCab|Cab|IR)$/;

/** Resolve a model's block art by its full name — the fallback for catalog rows
 *  with no FenderId (the Microphones). Returns null if the name isn't catalogued. */
export function resolveBlockArtByName(name: string): BlockArtSpec | null {
  return BY_NAME[name] ?? null;
}

// Resolve a device FenderId to a catalog id by stripping cab/IR/convolution suffixes one
// at a time via the canonical SUFFIX, CHECKING `inSet` BEFORE each strip — so an id
// already catalogued WITH a suffix (the `…CabIRConvRvb` reverb amps) matches directly and
// is never over-stripped, while a bare-catalogued amp discovered with an extra suffix
// (`ACD_HiwattDR103CanModCabIR` → `ACD_HiwattDR103CanMod`) still matches. The last-gap
// bridge appends `NoFx` once: a device "wet" amp id (…BlondeVibratoCabIRConvRvb) strips to
// a bare id (…BlondeVibrato) the catalog only carries WITH the NoFx token
// (…BlondeVibratoNoFx); NoFx is never stripped, so it must be re-added to match. Returns
// the first form satisfying `inSet`, else the fully-stripped id. The shared core of
// resolveBlockArt + resolveDeviceId (mirrored in the Rust `is_amp_model_id`), so the
// strip+NoFx rule lives in ONE place.
function resolveCatalogId(
  model: string,
  inSet: (id: string) => boolean,
): string {
  let m = model;
  for (let i = 0; i < 6; i++) {
    if (inSet(m)) return m;
    const next = m.replace(SUFFIX, "");
    if (next === m) break;
    m = next;
  }
  if (inSet(m)) return m;
  if (!m.endsWith("NoFx") && inSet(m + "NoFx")) return m + "NoFx";
  return m;
}

/** Resolve a device model id to its block art, or null if uncatalogued. */
export function resolveBlockArt(model: string): BlockArtSpec | null {
  return BY_ID[resolveCatalogId(model, (m) => Boolean(BY_ID[m]))] ?? null;
}

/** Terse model→caption fallback for an uncatalogued block — spaces the camelCase
 * id, never a raw mid-word slice. NON-uppercase: callers that want an uppercase
 * strip caption apply `.toUpperCase()` themselves (the Copy editor / EditGraph
 * naming paths rely on the cased form). */
export function shortFallback(model: string): string {
  return model
    .replace(/^(ACD_|USR_)/, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2");
}

/** The art-derived fields a signal-chain strip tile needs to render a block
 * through `BlockArt` — the SINGLE source the strip adapters share so they can't
 * drift from each other or from the Catalog (which feeds BlockArt the same set).
 * Returns a plain object (a structural superset of which is a `StripBlock`). */
export interface BlockArtFields {
  icon?: string;
  tone?: string;
  body?: string;
  panel?: string;
  footswitch?: "plate" | "metal" | "round";
  accent?: string;
  /** = `art.short`; the engine's caption + 1.8 dispatch token (undefined when
   *  uncatalogued — matches the Catalog's `lab={art?.short ?? ""}`). */
  lab?: string;
  /** visible strip caption, with an uppercase fallback for uncatalogued blocks. */
  name: string;
  /** the fuller Pro-Control-style model name (e.g. "HIWAY 105", "4X12 BRITISH
   *  V30") — shown on hover; the tile keeps the terse `name`/`lab` caption.
   *  Undefined when uncatalogued. */
  fullName?: string;
}

/** Map a device model id to its strip-tile art fields (resolves art once). */
export function blockArtTile(model: string): BlockArtFields {
  const art = resolveBlockArt(model);
  return {
    icon: art?.icon,
    tone: art?.tone,
    body: art?.body,
    panel: art?.panel,
    footswitch: art?.footswitch,
    accent: art?.accent,
    lab: art?.short,
    name: art?.short ?? shortFallback(model).toUpperCase(),
    fullName: art?.name,
  };
}

/** Which model id to look up strip-tile art for: a CabSim block names its tile
 *  from its actual cabinet (`ACD_<cabSimId>`) instead of the generic CAB IR;
 *  everything else (and a CabSim with no cab id) uses its own `model`. Shared by
 *  the hero strip, the Copy strip, and the dual-cab split so the resolution rule
 *  lives in one place. */
export function cabArtModel(
  cabSimId: string | undefined,
  model: string,
): string {
  return cabSimId != null && cabSimId !== "" ? `ACD_${cabSimId}` : model;
}

/** Head-over-cab art for an amp that carries its own cab (a combo / half-stack).
 *  `topIcon/topTone/topLab` = the amp head; `cabIcon/cabTone` = the cabinet. */
export interface HalfStackSpec {
  topIcon?: string;
  topTone?: string;
  topLab: string;
  cabIcon: string;
  cabTone?: string;
}

/** Build the head-over-cab spec from an amp head's ALREADY-RESOLVED art + the device's
 *  `cabsimid` (its built-in cab — a combo/half-stack like preset 003's HIWAY on a
 *  British 4×12). Takes the resolved `head` (not a model id) so the caller resolves
 *  the head art once. The cab art comes from the DEVICE's actual `cabsimid`, so the
 *  strip mirrors the unit. Returns `undefined` for a bare head (no cab id). */
export function ampCabHalfStack(
  head: BlockArtFields,
  cabSimId: string | undefined,
): HalfStackSpec | undefined {
  if (cabSimId == null || cabSimId === "") return undefined;
  const cab = blockArtTile(`ACD_${cabSimId}`);
  return {
    topIcon: head.icon,
    topTone: head.tone,
    topLab: head.lab ?? "",
    cabIcon: cab.icon ?? "cab4",
    cabTone: cab.tone,
  };
}

/** The art fields for a graph/edit node's strip tile, branching on node kind:
 *  the standalone CabSim block is NAMED from its cabinet; an amp carrying its own
 *  cab becomes a head-over-cab half-stack; everything else is its plain block art. */
export function nodeTileArt(
  model: string,
  cabSimId: string | undefined,
  // REQUIRED (no default) on purpose: a combo amp carries a cab_sim_id like a
  // half-stack head does, so every device-node caller must declare combo-ness
  // (`isComboBid(model)`) — a forgotten flag would silently re-stack combos, the
  // exact bug this fixes. Required → that omission is a compile error, not a regression.
  isCombo: boolean,
): BlockArtFields & { halfStack?: HalfStackSpec } {
  if (model === "ACD_CabSimTMS") {
    return blockArtTile(cabArtModel(cabSimId, model));
  }
  const tile = blockArtTile(model);
  // Cab-driven covering (tolex): the unit derives an amp's displayed covering from
  // amp_id + cabsimid (firmware DUBS_extender.json), so a blackface '65 Twin head on
  // the cream Creamback cab renders BLONDE. Mirror that — override the amp tile's tone
  // when the attached cab maps to a different covering (CAB_COVERING is a whitelist of
  // covering-changing amps; absent → keep the catalog tone). cabsimid arrives without
  // the ACD_ prefix, as the device sends it.
  const covering = CAB_COVERING[model]?.[(cabSimId ?? "").replace(/^ACD_/, "")];
  const base: BlockArtFields = covering ? { ...tile, tone: covering } : tile;
  // A combo's built-in speaker IS its cabsim — render the single combo tile, never a
  // synthesized head-over-cab stack. Heads carry no cab_sim_id so they never reach the
  // stack branch below; only combo-vs-half_stack needs disambiguating, and the caller
  // resolves it (blockArt must not import catalog → no form lookup here).
  if (isCombo) return base;
  const halfStack = ampCabHalfStack(base, cabSimId);
  return halfStack ? { ...base, halfStack } : base;
}

/** Resolve a device FenderId to its catalog id (see {@link resolveCatalogId}). */
export function resolveDeviceId(
  model: string,
  inSet: (id: string) => boolean,
): string {
  return resolveCatalogId(model, inSet);
}

export const HALF_STACK_PAIR: Record<string, string> = HALF_STACK_DEFAULTS;
