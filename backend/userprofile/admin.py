from django.contrib import admin

# Register your models here.

from userprofile.models import UserProfile


@admin.register(UserProfile)
class UserProfile_(admin.ModelAdmin):

    list_display = ("user", "displayname", "description")
