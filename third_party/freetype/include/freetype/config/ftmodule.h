/* The modules the game's FreeType 2.8.0 registers that matter for a TrueType face
 * (FT_Add_Default_Modules in UnityPlayer.dll: truetype, cff, psnames, pshinter, raster1, sfnt, smooth, otvalid). */
FT_USE_MODULE( FT_Driver_ClassRec, tt_driver_class )
FT_USE_MODULE( FT_Module_Class, psnames_module_class )
FT_USE_MODULE( FT_Module_Class, sfnt_module_class )
FT_USE_MODULE( FT_Renderer_Class, ft_smooth_renderer_class )
