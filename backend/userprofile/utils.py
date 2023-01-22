from django.db import models
from django.utils import timezone
import os
from uuid import uuid4


def path_and_rename(instance, filename):
    upload_to = "profile_pics"
    ext = filename.split(".")[-1]
    # get filename
    if instance.pk:
        filename = "{}.{}".format(instance.pk, ext)
    else:
        # set filename as random string
        filename = "{}.{}".format(uuid4().hex, ext)
    # return the whole path to the file
    return os.path.join(upload_to, filename)
