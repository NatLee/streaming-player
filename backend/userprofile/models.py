from email.policy import default
from django.db import models

# Create your models here.

from django.contrib.auth.models import User

class UserProfile(models.Model):
    """Customized user profile."""

    user = models.OneToOneField(User, on_delete=models.CASCADE, unique=True)
    displayname = models.CharField(max_length=50)
    description = models.TextField(blank=True, default='')

    def __str__(self):
        return f"{self.user.username} Profile"
